//! The `may_place_on` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0079

pub trait Hooks {
    /// On an authoritative server, `VegetationBlock.mayPlaceOn` for plant
    /// `state` on `ground` at `ground_pos`. The plant inherits vegetation rules
    /// from its template or opts into them in its block declaration. Only when
    /// the soil's `can_sustain_plant` leaves the decision to the plant does this
    /// method answer what it can stand on. `None` keeps its default rule.
    /// Answer only for your own blocks. The level is read-only.
    /// A disabled owner's subscribed rule preserves a plant of this type already
    /// at the position or settling there, but does not authorize a new planting.
    #[hook(
        side = "server",
        contract = "replace",
        out = "may",
        scope = "block",
        inert = "signal",
        key = "state",
        target = "sunrise_client::game::native_hooks::base::GameLevel<'_>::native_may_place_on",
        neoforge = "IBlockExtension.canSustainPlant"
    )]
    fn may_place_on();
}
