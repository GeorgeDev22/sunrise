//! A mod built by the build tool names each item of the game it uses; the real fixture loads on
//! the game's table, and is refused, naming the item, when one of those items changed.
#[path = "../src/test_stack.rs"]
mod test_stack;
use sunrise_mod_package::manifest::Package;
use sunrise_mod_wasm::{WasmPackage, load};

fn fixture() -> (WasmPackage, Vec<u8>) {
    let (Ok(fixture), Ok(std)) = (
        std::env::var("SUNRISE_WASM_FIXTURE"),
        std::env::var("SUNRISE_WASM_STD"),
    ) else {
        panic!(
            "SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD must be set: run with the wasm-mods workflow's environment"
        );
    };
    let path = std::path::Path::new(&fixture);
    let mut package = WasmPackage {
        manifest: Package::read(path).unwrap().manifest,
        module: Package::read_wasm(path).unwrap(),
    };
    package.manifest.inject.clear();
    (package, std::fs::read(std).unwrap())
}

/// The module with the last digit of the first name that starts `prefix` changed: the name keeps
/// its length, so no section moves.
fn changed(module: &[u8], prefix: &[u8]) -> (Vec<u8>, String) {
    let start = module
        .windows(prefix.len())
        .position(|window| window == prefix)
        .expect("the fixture imports such a name");
    let end = start
        + module[start..]
            .iter()
            .position(|&byte| byte == b'#')
            .unwrap()
        + 33;
    let mut out = module.to_vec();
    out[end - 1] = if out[end - 1] == b'0' { b'1' } else { b'0' };
    (out, String::from_utf8(module[start..end].to_vec()).unwrap())
}

/// Every import of module `sunrise` the module names, in the import section's order.
fn sunrise_imports(module: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(module) {
        if let wasmparser::Payload::ImportSection(section) = payload.unwrap() {
            for import in section.into_imports() {
                let import = import.unwrap();
                if import.module == "sunrise" {
                    found.push(import.name.to_owned());
                }
            }
            break;
        }
    }
    found
}

/// The module with the last digit of the import `name` changed, wherever it is in the section.
fn changed_import(module: &[u8], name: &str) -> Vec<u8> {
    let start = module
        .windows(name.len())
        .position(|window| window == name.as_bytes())
        .expect("the module imports the name");
    let end = start + name.len();
    let mut out = module.to_vec();
    out[end - 1] = if out[end - 1] == b'0' { b'1' } else { b'0' };
    out
}

#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn the_fixture_loads_on_the_games_table() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        let (package, std) = fixture();
        let loaded = load(&std, vec![package]);
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        assert_eq!(loaded.mods.len(), 1);
    })
}

#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn a_changed_call_the_fixture_uses_refuses_it_by_name() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        for prefix in [&b"call:"[..], b"hook:", b"part:"] {
            let (mut package, std) = fixture();
            let (module, name) = changed(&package.module, prefix);
            package.module = module;
            let loaded = load(&std, vec![package]);
            assert!(loaded.mods.is_empty());
            let reason = loaded.refused[0].1.to_string();
            let item = name.split('#').next().unwrap();
            assert!(
                reason.contains(item.trim_start_matches("call:")) && reason.contains("changed"),
                "{name}: {reason}"
            );
            assert!(
                reason.ends_with("(or a type it carries changed)"),
                "{reason}"
            );
        }
    })
}

/// Every hook, Part method, Part subscription and point the fixture's source implements or
/// configures, as it is named without its hash, written out by hand from the fixture's sources:
/// the import section names exactly these, so losing one (or gaining one nothing wrote) fails.
const HOOKS: &[&str] = &[
    "hook:Hooks::baby_entity_spawn@2",
    "hook:Hooks::blocks@1",
    "hook:Hooks::chunk_data_load@1",
    "hook:Hooks::chunk_data_save@1",
    "hook:Hooks::client_packet_send@1",
    "hook:Hooks::entity_interact@2",
    "hook:Hooks::entity_tick_listened@1",
    "hook:Hooks::entity_tick_post@1",
    "hook:Hooks::entity_tick_pre@1",
    "hook:Hooks::interaction_key_mapping_triggered@2",
    "hook:Hooks::living_death@2",
    "hook:Hooks::living_entity_use_item_tick@1",
    "hook:Hooks::menu_links@1",
    "hook:Hooks::mob_split@2",
    "hook:Hooks::movement_input@2",
    "hook:Hooks::player_stat_award@2",
    "hook:Hooks::recipe_sources@1",
    "hook:Hooks::register",
    "hook:Hooks::register_commands@2",
    "hook:Hooks::register_json_animation_types@1",
    "hook:Hooks::render_gui_pre@1",
    "hook:Hooks::render_level_stage@1",
    "hook:Hooks::right_click_block@1",
    "hook:Hooks::run_mod_command@1",
    "hook:Hooks::server_started@1",
    "hook:Hooks::server_stopped@1",
    "hook:Hooks::server_tick@1",
    "hook:Hooks::standalone_models@2",
    "hook:Hooks::subscribed",
    "hook:Hooks::tick_target_millis@1",
    "hook:Hooks::use_item_on_block@1",
];
const PARTS: &[&str] = &[
    "part:BlockEntityPart::block_entity_subscribed",
    "part:BlockEntityPart::block_entity_tick",
    "part:BlockPart::block_path_type",
    "part:BlockPart::block_subscribed",
    "part:BlockPart::collision_shape",
    "part:BlockPart::contents_changed",
    "part:BlockPart::explosion_resistance",
    "part:BlockPart::explosion_resistance_is_pure",
    "part:BlockPart::on_block_state_change",
    "part:EntityPart::entity_subscribed",
    "part:EntityPart::general_entity_tick",
    "part:ItemPart::ask_can_equip",
    "part:ItemPart::can_equip",
    "part:ItemPart::item_interact_mob",
    "part:ItemPart::item_subscribed",
    "part:ItemPart::prepare_can_equip",
    "part:ItemPart::release_can_equip",
    "part:ItemPart::use_item",
    "part:ItemPart::use_on",
];
const POINTS: &[&str] = &[
    "point:Points::game_beacon_average",
    "point:Points::game_commands_wrap_degrees",
    "point:Points::game_server_lifecycle_remember_shutdown",
    "point:Points::listener_fixture_mirrors",
    "point:Points::listener_fixture_sum_return",
    "point:Points::tick",
];

/// A mark names an item once per item: a mod's several hooks, the several methods of a Part, its
/// subscription and its injection points each appear in the import section, and a changed one is
/// refused by name whichever of them it is, not only the first of its kind.
#[test]
#[ignore = "daily"]
fn every_hook_part_and_point_the_fixture_implements_is_imported_and_checked() {
    let (package, _) = fixture();
    let imports = sunrise_imports(&package.module);
    let marked: std::collections::BTreeSet<&str> = imports
        .iter()
        .filter(|name| {
            ["hook:", "part:", "point:"]
                .iter()
                .any(|kind| name.starts_with(kind))
        })
        .map(|name| name.split('#').next().unwrap())
        .collect();
    let expected: std::collections::BTreeSet<&str> =
        HOOKS.iter().chain(PARTS).chain(POINTS).copied().collect();
    assert_eq!(
        marked.difference(&expected).collect::<Vec<_>>(),
        Vec::<&&str>::new(),
        "imported, but nothing in the fixture writes it"
    );
    assert_eq!(
        expected.difference(&marked).collect::<Vec<_>>(),
        Vec::<&&str>::new(),
        "written in the fixture, but not imported"
    );
    for name in imports.iter().filter(|name| {
        ["hook:", "part:", "point:"]
            .iter()
            .any(|kind| name.starts_with(kind))
    }) {
        let module = changed_import(&package.module, name);
        let refused = sunrise_mod_wasm::fixture::mismatches_with_added_call(
            &module,
            "call:Host::a_method_added_after_this_mod_was_built#00112233445566778899aabbccddeeff",
        )
        .unwrap();
        let item = name.split('#').next().unwrap();
        assert!(
            refused.len() == 1 && refused[0].starts_with(&format!("{item} changed")),
            "a changed {name} is not refused by name: {refused:?}"
        );
    }
}

/// A mod built before a method was added to the API loads on the game that has it: the table is
/// searched by name, so an entry no import names changes nothing for the mod.
#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn an_unrelated_host_method_added_to_the_game_refuses_nothing() {
    let (package, _) = fixture();
    let imports_before = sunrise_mod_wasm::fixture::mismatches_with_added_call(
        &package.module,
        "call:Host::a_method_added_after_this_mod_was_built#00112233445566778899aabbccddeeff",
    )
    .unwrap();
    assert!(imports_before.is_empty(), "{imports_before:?}");
    // The same table with a changed method the mod uses is refused, so the check above bites.
    let (module, _) = changed(&package.module, b"call:");
    let refused = sunrise_mod_wasm::fixture::mismatches_with_added_call(
        &module,
        "call:Host::a_method_added_after_this_mod_was_built#00112233445566778899aabbccddeeff",
    )
    .unwrap();
    assert_eq!(refused.len(), 1, "{refused:?}");
}

/// A mod built before this change, by the SDK of the base commit 36839e7ce7d (the one numbered
/// `host` import and the whole-API layout export), is refused once, with the rebuild text.
#[test]
#[ignore = "needs SUNRISE_WASM_OLD_FIXTURE and SUNRISE_WASM_OLD_STD: the fixture and std built by the SDK before the per-item imports"]
fn a_mod_built_before_the_break_is_refused_once_with_the_rebuild_text() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        let (Ok(old), Ok(std)) = (
            std::env::var("SUNRISE_WASM_OLD_FIXTURE"),
            std::env::var("SUNRISE_WASM_OLD_STD"),
        ) else {
            panic!("SUNRISE_WASM_OLD_FIXTURE and SUNRISE_WASM_OLD_STD must be set");
        };
        let path = std::path::Path::new(&old);
        let mut package = WasmPackage {
            manifest: Package::read(path).unwrap().manifest,
            module: Package::read_wasm(path).unwrap(),
        };
        package.manifest.inject.clear();
        sunrise_mod_wasm::set_game_build("0123abcd");
        let loaded = load(&std::fs::read(std).unwrap(), vec![package]);
        assert!(loaded.mods.is_empty());
        assert_eq!(loaded.refused.len(), 1, "{:?}", loaded.refused);
        let reason = loaded.refused[0].1.to_string();
        assert!(
            reason.starts_with("built for an older Sunrise, rebuild with SDK sdk-0123abcd: "),
            "{reason}"
        );
    })
}
