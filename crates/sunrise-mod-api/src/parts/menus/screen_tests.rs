use super::*;
/// A part that lays a screen out.
struct Layout;
#[mod_api::sunrise_part]
impl MenuPart for Layout {
    fn screen_draw(&self, _: &dyn ClientHost, _: &mut ScreenDrawEvent<'_>) {}
}
/// Another, of another kind.
struct Overlay;
#[mod_api::sunrise_part]
impl MenuPart for Overlay {
    fn screen_draw(&self, _: &dyn ClientHost, _: &mut ScreenDrawEvent<'_>) {}
}
/// A part on a menu type that lays out no screen.
struct Rules;
#[mod_api::sunrise_part]
impl MenuPart for Rules {
    fn menu_still_valid(&self, _: &dyn LoopContext, _: &mut MenuStillValidEvent<'_>) {}
}
#[test]
fn a_second_screen_part_for_the_same_screens_is_refused() {
    let mut registrar = PartRegistrar::new("alpha");
    let r: &mut dyn Registrar = &mut registrar;
    r.menu_part("alpha:pack", Layout);
    r.menu_part("minecraft:hopper", Layout);
    // One screen part on a mod's menu type, whoever attaches the second.
    r.menu_part("alpha:pack", Overlay);
    // A part that lays out no screen sits beside it.
    r.menu_part("alpha:pack", Rules);
    registrar.mod_id = "beta";
    let r: &mut dyn Registrar = &mut registrar;
    r.menu_part("alpha:pack", Overlay);
    // A vanilla type takes one per mod: beta's blocks get beta's.
    r.menu_part("minecraft:hopper", Overlay);
    registrar.mod_id = "alpha";
    let r: &mut dyn Registrar = &mut registrar;
    r.menu_part("minecraft:hopper", Overlay);
    // Kinds are named by their full type names.
    let here = concat!(module_path!(), "::");
    let conflicts = [
        ("alpha:pack", "alpha", "alpha"),
        ("alpha:pack", "beta", "alpha"),
        ("minecraft:hopper", "alpha", "alpha"),
    ];
    assert_eq!(registrar.refused.len(), conflicts.len());
    for (refusal, (menu, rival, owner)) in registrar.refused.iter().zip(conflicts) {
        for name in [
            menu,
            &format!("Overlay of {rival}"),
            &format!("Layout of {owner}"),
        ] {
            assert!(refusal.contains(name), "{refusal} does not name {name}");
        }
    }
    let owners = |menu: &str| -> Vec<_> {
        registrar.parts.menus[menu]
            .iter()
            .map(|p| (p.mod_id, p.type_name.trim_start_matches(here)))
            .collect()
    };
    assert_eq!(
        owners("alpha:pack"),
        [("alpha", "Layout"), ("alpha", "Rules")]
    );
    assert_eq!(
        owners("minecraft:hopper"),
        [("alpha", "Layout"), ("beta", "Overlay")]
    );
}
