//! The compiled wasm part answers a collision query like the same part natively, and what a
//! dispatch to each costs. Run with the fixture and std built as `bench.rs` describes:
//! `cargo test --release -p sunrise-mod-wasm --test collision_shape -- --ignored --nocapture`.
#[path = "fixture/src/collision_shape.rs"]
mod fixture;
#[path = "../src/test_stack.rs"]
mod test_stack;
use mod_api::{BlockMetadata, BlockState, CollisionEntityKind, CollisionQuery, LoopContext};
use std::{sync::Arc, time::Instant};
use sunrise_mod_package::manifest::Package;

struct Context;
impl LoopContext for Context {
    fn side(&self) -> mod_api::LoopSide {
        mod_api::LoopSide::Server
    }
    fn registry(&self, _: &str) -> Option<Arc<[String]>> {
        None
    }
    fn setting(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn mods(&self) -> mod_api::LoadedMods {
        unreachable!("not used by this test")
    }
    fn mod_recipe_property_contains(&self, _: &str, _: mod_api::ItemKey) -> bool {
        false
    }
    fn is_in_tag(&self, _: mod_api::ItemKey, _: mod_api::TagKey) -> Option<bool> {
        None
    }
    fn item_count(&self, _: mod_api::TagKey) -> Option<usize> {
        None
    }
    fn item_at(&self, _: mod_api::TagKey, _: usize) -> Option<mod_api::ItemKey> {
        None
    }
}

struct Native;
#[mod_api::__subscriptions]
impl mod_api::Hooks for Native {
    fn register(&self, r: &mut dyn mod_api::Registrar, cx: &mod_api::RegistrationContext) {
        let stone = r.block_key("minecraft:stone");
        r.block_part(stone, fixture::SlabForSeven);
        let _ = cx;
    }
}

/// The stone state, with the block keys resolved as the fixture's other tests do.
fn stone() -> BlockState {
    mod_api::__private::resolve_block_keys(
        [
            "minecraft:stone",
            "minecraft:air",
            "minecraft:water_cauldron",
            "wasm_fixture:part_state",
        ],
        |_| {
            sunrise_mod_runtime::block_definitions::BlockDefinition::new(
                &[&std::collections::BTreeMap::new()],
                0,
            )
            .map_err(|error| format!("{error:?}"))
        },
    )
    .unwrap();
    let metadata = mod_api::__private::block_definitions().unwrap();
    BlockState::from(
        metadata
            .of(
                mod_api::__private::part_tests::block_key("minecraft:stone"),
                &[],
            )
            .unwrap(),
    )
}

fn resolve() {
    mod_api::__private::resolve_parts(
        &Default::default(),
        &Default::default(),
        mod_api::__private::KnownIds {
            block: &|id| matches!(id, "minecraft:stone" | "minecraft:air"),
            item: &|_| false,
        },
    )
    .unwrap();
}

/// Checks the part's answers, then returns the mean ns of 100,000 dispatches for entity 7.
fn measure(label: &str, state: BlockState) -> u128 {
    let pos = mod_api::BlockPos::new(1, 64, 1);
    let ask = |entity: u32, descending: bool| {
        let query = CollisionQuery::new(Some(entity), CollisionEntityKind::Player, Some(65.0))
            .with_descending(descending);
        let mut shape = None;
        mod_api::dispatch_collision_shape(&Context, pos, state, &query, &mut shape);
        shape
    };
    assert_eq!(
        ask(7, false),
        Some(vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]),
        "{label}"
    );
    assert_eq!(
        ask(8, false),
        None,
        "{label}: another entity keeps the shape"
    );
    assert_eq!(ask(7, true), None, "{label}: the part reads descending");
    let mut samples = Vec::new();
    for round in 0..30 {
        let now = Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(ask(std::hint::black_box(7), false));
        }
        if round >= 5 {
            samples.push(now.elapsed().as_nanos() / 100_000);
        }
    }
    samples.sort_unstable();
    let mean = samples.iter().sum::<u128>() / samples.len() as u128;
    println!(
        "{label}: mean {mean} ns per dispatch, p95 {} ns",
        samples[samples.len() * 95 / 100]
    );
    mean
}

#[test]
#[ignore = "daily"]
fn a_compiled_part_answers_a_collision_query_like_the_native_one() {
    test_stack::on_guest_stack(|| {
        let path =
            std::env::var("SUNRISE_WASM_FIXTURE").expect("build tests/fixture from this tree");
        let std = std::env::var("SUNRISE_WASM_STD").expect("build this tree's shared wasm std");
        let path = std::path::Path::new(&path);
        let mut manifest = Package::read(path).unwrap().manifest;
        manifest.inject.clear();
        let info = |id: &str| {
            mod_api::PackageInfo::new(
                id.into(),
                "0.1.0".into(),
                mod_api::PackageKind::Code,
                mod_api::Side::Both,
                vec![id.into()],
                Vec::new(),
            )
        };
        let infos = [info(&manifest.id), info("collision_shape_probe")];
        let loaded = sunrise_mod_wasm::load(
            &std::fs::read(std).unwrap(),
            vec![sunrise_mod_wasm::WasmPackage {
                manifest,
                module: Package::read_wasm(path).unwrap(),
            }],
        );
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        loaded.initialize(&infos).unwrap();
        // The fixture guest declares a general entity type in `register`, which only the registration
        // phase accepts. The scope follows `initialize`: the phase lives in the backend it installs.
        mod_api::__private::initialize().unwrap();
        let _registration =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        mod_api::testing::with_mods(loaded.mods.into(), || {
            mod_api::__private::install_packages(&infos, &[]);
            mod_api::__private::part_tests::install_parts_fixture(mod_api::__private::mods())
                .unwrap();
            mod_api::__private::mark_game_thread();
            let state = stone();
            mod_api::__private::resolve_parts(
                &Default::default(),
                &Default::default(),
                mod_api::__private::KnownIds {
                    block: &|id| {
                        matches!(
                            id,
                            "minecraft:stone"
                                | "minecraft:air"
                                | "minecraft:water_cauldron"
                                | "wasm_fixture:part_state"
                        )
                    },
                    item: &|id| matches!(id, "minecraft:paper" | "minecraft:snowball"),
                },
            )
            .unwrap();
            let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            measure("wasm part", state);
        });
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new("collision_native", Box::new(Native))].into(),
            || {
                mod_api::__private::part_tests::install_parts_fixture(mod_api::__private::mods())
                    .unwrap();
                mod_api::__private::mark_game_thread();
                let state = stone();
                resolve();
                let _phase =
                    mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
                measure("native part", state);
            },
        );
    });
}
