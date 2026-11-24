//! Placement modifier types a mod registers (vanilla's
//! `PlacementModifierType` with its own `PlacementModifier`), handed over once
//! at startup and looked up while a placed feature decodes, after the vanilla
//! placement modifier types. A decoded modifier is
//! [`PlacementModifier::Mod`](super::PlacementModifier::Mod): the placement
//! chain runs [`ModPlacementModifier::modify`] for each incoming position. Runs on worldgen worker threads, so a modifier keeps no mutable
//! state and its contexts carry no `Host`.
use super::{PlacementContext, PlacementError, PlacementModifier, PlacementQueryError};
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};
use crate::worldgen::DecodeError;
use crate::worldgen::block_predicate::PredicateBlockResolver;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};
use sunrise_core::position::BlockPos;
use sunrise_core::random::{
    InvalidBound, LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource,
};

/// Decode-time context passed to [`ModPlacementModifierType::decode`]: carries
/// the block resolver (for `BlockRegistry` lookup) and enforces depth and node
/// budgets across nested modifier decodes.
pub struct PlacementDecodeScope<'a> {
    depth: usize,
    remaining: Cell<usize>,
    blocks: Option<&'a dyn PredicateBlockResolver>,
}
impl<'a> PlacementDecodeScope<'a> {
    pub(super) fn new(
        depth: usize,
        remaining: usize,
        blocks: Option<&'a dyn PredicateBlockResolver>,
    ) -> Self {
        Self {
            depth,
            remaining: Cell::new(remaining),
            blocks,
        }
    }
    pub(super) fn remaining(&self) -> usize {
        self.remaining.get()
    }
    /// The numeric block registry from the active block resolver, if any.
    pub fn block_registry(&self) -> Option<&'a BlockRegistry> {
        self.blocks.and_then(|b| b.block_registry())
    }
    /// Decode a nested placement modifier of any type, vanilla's or a mod's.
    pub fn decode_modifier(&self, json: &str) -> Result<PlacementModifier, DecodeError> {
        let mut remaining = self.remaining.get();
        let modifier = PlacementModifier::decode_with_blocks(
            json,
            self.depth + 1,
            &mut remaining,
            self.blocks,
        );
        self.remaining.set(remaining);
        modifier
    }
}

/// A placement modifier type's engine side: the codec of vanilla's
/// `PlacementModifierType`, as the executable's adapter sees it.
pub trait ModPlacementModifierType: Send + Sync {
    /// Decode one placement modifier from its JSON object (`type` included).
    /// An error fails the placed feature's decode.
    fn decode(
        &self,
        config: &serde_json::Value,
        scope: &PlacementDecodeScope<'_>,
    ) -> Result<Arc<dyn ModPlacementModifier>, String>;
}

/// A decoded placement modifier of a mod's type: vanilla's
/// `PlacementModifier`. `Send + Sync` because neighbouring chunks decorate at
/// once on worker threads; the same random gives the same positions.
pub trait ModPlacementModifier: Send + Sync + std::fmt::Debug + std::any::Any {
    /// `PlacementModifier.modify(context, random, origin, output)`: pass zero
    /// or more positions to `output`, returning the first error it gives.
    fn modify(
        &self,
        context: &mut dyn ModPlacementContext,
        random: &mut dyn mod_api::RandomSource,
        origin: [i32; 3],
        output: &mut dyn FnMut([i32; 3]) -> Result<(), PlacementQueryError>,
    ) -> Result<(), PlacementQueryError>;
}

/// Read-only world and nested-modifier queries available to
/// [`ModPlacementModifier::modify`].
pub trait ModPlacementContext {
    fn block_registry(&self) -> Option<&'static BlockRegistry>;
    fn world_seed(&mut self) -> Option<i64>;
    fn min_y(&mut self) -> Option<i32>;
    fn max_y(&mut self) -> Option<i32>;
    fn sea_level(&mut self) -> Option<i32>;
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId>;
    fn block_in_tag(&mut self, pos: [i32; 3], tag: &str) -> Option<bool>;
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    fn biome(&mut self, pos: [i32; 3]) -> Option<mod_api::BiomeKey>;
    /// Run a decoded nested placement modifier on `origin`, sharing `random`
    /// and the current placement budget, passing each position it produces to
    /// `output`.
    fn apply_modifier(
        &mut self,
        modifier: &PlacementModifier,
        random: &mut dyn mod_api::RandomSource,
        origin: [i32; 3],
        output: &mut dyn FnMut([i32; 3]) -> Result<(), PlacementQueryError>,
    ) -> Result<(), PlacementQueryError>;
}

/// Adapts an engine `R: RandomSource` into `mod_api::RandomSource`. The
/// placement chain's output closure needs the same random while the mod holds
/// this adapter, so both share it through a `RefCell`; each borrow lasts one
/// call and neither side calls the other while it holds one.
pub(super) struct EngineRandom<'a, 'r, R>(pub(super) &'a RefCell<&'r mut R>);
impl<R: RandomSource> mod_api::RandomSource for EngineRandom<'_, '_, R> {
    fn next_int(&mut self) -> i32 {
        self.0.borrow_mut().next_int()
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            0
        } else {
            self.0.borrow_mut().next_int_bound(bound).unwrap_or(0)
        }
    }
    fn next_long(&mut self) -> i64 {
        self.0.borrow_mut().next_long()
    }
    fn next_boolean(&mut self) -> bool {
        self.0.borrow_mut().next_boolean()
    }
    fn next_float(&mut self) -> f32 {
        self.0.borrow_mut().next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.0.borrow_mut().next_double()
    }
    fn next_gaussian(&mut self) -> f64 {
        self.0.borrow_mut().next_gaussian()
    }
}

/// Adapts a `&mut dyn mod_api::RandomSource` back into `sunrise_core::random::RandomSource`
/// for nested `PlacementModifier::modify_inner` calls.
enum ApiRandom<'a> {
    Borrowed(&'a mut dyn mod_api::RandomSource),
    Forked(LegacyRandom),
}
struct ApiPositional<'a>(LegacyPositionalRandom, PhantomData<&'a ()>);
impl<'a> PositionalRandom for ApiPositional<'a> {
    type Source = ApiRandom<'a>;
    fn at(&self, position: BlockPos) -> Self::Source {
        ApiRandom::Forked(self.0.at(position))
    }
    fn from_seed(&self, seed: i64) -> Self::Source {
        ApiRandom::Forked(self.0.from_seed(seed))
    }
    fn from_hash_of(&self, name: &str) -> Self::Source {
        ApiRandom::Forked(self.0.from_hash_of(name))
    }
    fn from_hash_of_utf16(&self, name: &[u16]) -> Self::Source {
        ApiRandom::Forked(self.0.from_hash_of_utf16(name))
    }
}
impl<'a> RandomSource for ApiRandom<'a> {
    type Positional = ApiPositional<'a>;
    fn set_seed(&mut self, seed: i64) {
        *self = Self::Forked(LegacyRandom::new(seed));
    }
    fn next_int(&mut self) -> i32 {
        match self {
            Self::Borrowed(r) => r.next_int(),
            Self::Forked(r) => r.next_int(),
        }
    }
    fn next_int_bound(&mut self, bound: i32) -> Result<i32, InvalidBound> {
        if bound <= 0 {
            return Err(InvalidBound);
        }
        match self {
            // A mod's own `RandomSource` may answer outside `0..bound`; the
            // engine indexes with the answer, so refuse it here.
            Self::Borrowed(r) => {
                let value = r.next_int_bound(bound);
                if (0..bound).contains(&value) {
                    Ok(value)
                } else {
                    Err(InvalidBound)
                }
            }
            Self::Forked(r) => r.next_int_bound(bound),
        }
    }
    fn next_long(&mut self) -> i64 {
        match self {
            Self::Borrowed(r) => r.next_long(),
            Self::Forked(r) => r.next_long(),
        }
    }
    fn next_boolean(&mut self) -> bool {
        match self {
            Self::Borrowed(r) => r.next_boolean(),
            Self::Forked(r) => r.next_boolean(),
        }
    }
    fn next_float(&mut self) -> f32 {
        match self {
            Self::Borrowed(r) => r.next_float(),
            Self::Forked(r) => r.next_float(),
        }
    }
    fn next_double(&mut self) -> f64 {
        match self {
            Self::Borrowed(r) => r.next_double(),
            Self::Forked(r) => r.next_double(),
        }
    }
    fn next_gaussian(&mut self) -> f64 {
        match self {
            Self::Borrowed(r) => r.next_gaussian(),
            Self::Forked(r) => r.next_gaussian(),
        }
    }
    fn fork_positional(&mut self) -> Self::Positional {
        ApiPositional(
            LegacyRandom::new(self.next_long()).fork_positional(),
            PhantomData,
        )
    }
    fn fork(&mut self) -> Self {
        Self::Forked(LegacyRandom::new(self.next_long()))
    }
}

pub(super) struct PlacementRunContext<'a, C> {
    context: &'a mut C,
    remaining: &'a Cell<usize>,
}
impl<'a, C> PlacementRunContext<'a, C> {
    pub(super) fn new(context: &'a mut C, remaining: &'a Cell<usize>) -> Self {
        Self { context, remaining }
    }
}
impl<C: PlacementContext> ModPlacementContext for PlacementRunContext<'_, C> {
    fn block_registry(&self) -> Option<&'static BlockRegistry> {
        self.context.block_registry()
    }
    fn world_seed(&mut self) -> Option<i64> {
        self.context.world_seed().ok()
    }
    fn min_y(&mut self) -> Option<i32> {
        self.context.min_y().ok()
    }
    fn max_y(&mut self) -> Option<i32> {
        self.context.max_y().ok()
    }
    fn sea_level(&mut self) -> Option<i32> {
        self.context.sea_level().ok()
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockStateId> {
        self.context.block_state(pos).ok()
    }
    fn block_in_tag(&mut self, pos: [i32; 3], tag: &str) -> Option<bool> {
        self.context.block_matches_tag(pos, tag).ok()
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        self.context.height(kind, x, z).ok()
    }
    fn biome(&mut self, pos: [i32; 3]) -> Option<mod_api::BiomeKey> {
        self.context.biome_key(pos).ok()
    }
    fn apply_modifier(
        &mut self,
        modifier: &PlacementModifier,
        random: &mut dyn mod_api::RandomSource,
        origin: [i32; 3],
        output: &mut dyn FnMut([i32; 3]) -> Result<(), PlacementQueryError>,
    ) -> Result<(), PlacementQueryError> {
        let mut random = ApiRandom::Borrowed(random);
        let start = self.remaining.get();
        let mut nested = start;
        let result = modifier.modify_inner(
            self.context,
            &mut random,
            origin,
            &mut |_, pos| output(pos),
            &mut nested,
        );
        // `output` may spend from the same budget while the nested modifier
        // runs, so take only the nested modifier's own spending off what is left.
        self.remaining
            .set(self.remaining.get().saturating_sub(start - nested));
        match result {
            Ok(()) => Ok(()),
            Err(PlacementError::World(err) | PlacementError::Output(err)) => Err(err),
            Err(PlacementError::WorkLimit) => Err(PlacementQueryError::WorkLimit),
            Err(PlacementError::Random(err)) => Err(PlacementQueryError::Failed(err.to_string())),
        }
    }
}

/// The installed types, `modid:below_sea_level` → type. Set once, before any
/// world opens (the executable hands the mods' registrations over), and read
/// while placed features decode.
pub type ModPlacementModifierTypes = BTreeMap<String, Arc<dyn ModPlacementModifierType>>;

static MOD_TYPES: OnceLock<ModPlacementModifierTypes> = OnceLock::new();

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// The types a test installs, in place of the process-wide table.
    static TEST_TYPES: std::cell::RefCell<Option<ModPlacementModifierTypes>> =
        const { std::cell::RefCell::new(None) };
}

/// Install the process-wide table once; a second installation keeps the first
/// and hands its own types back.
pub fn install_mod_placement_modifier_types(
    types: ModPlacementModifierTypes,
) -> Result<(), ModPlacementModifierTypes> {
    MOD_TYPES.set(types)
}

/// The installed placement modifier type `id`: none until the executable
/// installs the mods', and a test's own while one is installed.
pub fn mod_placement_modifier_type(id: &str) -> Option<Arc<dyn ModPlacementModifierType>> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(found) =
        TEST_TYPES.with(|cell| cell.borrow().as_ref().map(|types| types.get(id).cloned()))
    {
        return found;
    }
    MOD_TYPES.get()?.get(id).cloned()
}

/// Runs `with` over a test's own placement modifier types, as decoding reads
/// them. Restores the previous scoped table even if `with` panics.
#[cfg(any(test, feature = "test-support"))]
pub fn with_test_placement_modifier_types<T>(
    types: &ModPlacementModifierTypes,
    with: impl FnOnce() -> T,
) -> T {
    struct Guard(Option<ModPlacementModifierTypes>);
    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_TYPES.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let prev = TEST_TYPES.with(|cell| cell.borrow_mut().replace(types.clone()));
    let _guard = Guard(prev);
    with()
}
