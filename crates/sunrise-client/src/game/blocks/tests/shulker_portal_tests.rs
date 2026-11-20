//! A shulker's box grows toward its open lid (`Shulker.makeBoundingBox`), so
//! `Entity.checkInsideBlocks` finds a nether portal the lid opens into, beside
//! the shell as well as above it.

use super::*;
use crate::game::game_tests::tests::server_game;
use sunrise_client::motion::PlayerMotion;

#[test]
#[ignore = "daily"]
fn a_shulker_on_a_wall_goes_through_the_portal_its_lid_opens_into() {
    let mut resources = None;
    let mut game = server_game(&mut resources);
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap());
    game.load_ticking_neighbourhood(0, 0);
    let registry = game.pack.assets.registry;
    let stone = registry.default_state("minecraft:stone").unwrap();
    let portal = registry.default_state("minecraft:nether_portal").unwrap();
    // Two shells clinging to stone on their west (attach face WEST, 4), so
    // their lids open east, into a portal block: one wide open, one shut.
    for (z, peek) in [(4, 100), (8, 0)] {
        game.world.set(3, 64, z, stone).unwrap();
        game.world.set(5, 64, z, portal).unwrap();
        let tag = Tag::compound([
            ("id", Tag::String("minecraft:shulker".into())),
            ("Pos", doubles([4.5, 64.0, f64::from(z) + 0.5])),
            ("AttachFace", Tag::Byte(4)),
            ("Peek", Tag::Byte(peek)),
        ]);
        game.load_listed("minecraft:shulker", None, None, tag)
            .unwrap();
    }
    assert_eq!(game.mobs.len(), 2);
    // The sweep sees the box last tick's peek built (`Shulker.tick` grows it
    // after `super.tick()`); these shells have not ticked, so give each the
    // lid it loaded with as last tick's too.
    for m in game.mobs.iter_mut() {
        m.anim.vo = m.anim.v;
    }
    game.update_chunk_tickets();
    assert!(
        game.entity_ticking_area()(4, 4),
        "chunk (0, 0) ticks entities"
    );
    for _ in 0..3 {
        game.handle_entity_portals();
        game.find_entity_portals();
    }
    let left: Vec<_> = game.mobs.iter().map(|m| m.position.z).collect();
    assert_eq!(
        left,
        vec![8.5],
        "the open shell's box reached the portal and crossed; the shut one stayed"
    );
}
