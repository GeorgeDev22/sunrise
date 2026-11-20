use crate::game::{
    Game, native_entity_refs::tests::with_client_game, native_lifecycle::NativeLifecycle,
};
use sunrise_client::{level_loading::Reason, motion::PlayerMotion};
use sunrise_core::geometry::Vec3;
use sunrise_protocol::{
    connection::Pipe,
    packets::play::{self, Clientbound as C, SpawnInfo},
};

/// A logged-in client whose player stands at the origin, looking at yaw 45, pitch 20.
fn client_at_origin(game: &mut Game<'_>) {
    game.native_lifecycle = NativeLifecycle::default();
    let (client, _server) = Pipe::pair();
    game.connect_local(client, "LookAtTest").unwrap();
    let spawn = SpawnInfo {
        dimension_type: 0,
        dimension: "minecraft:overworld".into(),
        seed: 0,
        game_type: 1,
        previous_game_type: None,
        debug: false,
        flat: true,
        last_death: None,
        portal_cooldown: 0,
        sea_level: 63,
    };
    game.enter_remote_dimension(&spawn, false, Reason::Other);
    let remote = game.remote.as_mut().unwrap();
    remote.logged_in = true;
    remote.entity_id = 7;
    let mut player = PlayerMotion::new(game.motion, Vec3::ZERO).unwrap();
    player.yaw = 45.0;
    player.head_yaw = 45.0;
    game.player = Some(player);
    (game.yaw, game.pitch) = (45.0, 20.0);
    game.rope_rot_old = (45.0, 20.0);
}

fn look_at(game: &mut Game<'_>, target: [f64; 3], entity: Option<i32>) {
    game.handle_packet(C::PlayerLookAt(play::PlayerLookAt {
        from: 0,
        target,
        entity: entity.map(|id| play::LookAtEntity { id, anchor: 0 }),
    }))
    .unwrap();
}

#[test]
#[ignore = "daily"]
fn look_at_turns_to_any_tracked_entity_and_snaps_the_old_rotation() {
    with_client_game(|game| {
        client_at_origin(game);
        let arrow = game
            .remote
            .as_ref()
            .unwrap()
            .builtin
            .get("minecraft:entity_type")
            .unwrap()
            .id("minecraft:arrow")
            .unwrap();
        game.handle_packet(C::AddEntity(play::AddEntity {
            id: 40,
            uuid: 11,
            kind: arrow,
            position: [3.0, 0.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
            data: 0,
        }))
        .unwrap();

        // The arrow, due +x, wins over the packet's point behind the player.
        look_at(game, [-3.0, 0.0, 0.0], Some(40));
        assert_eq!(
            (game.yaw, game.pitch),
            (-90.0, 0.0),
            "faces the arrow, not the point"
        );
        assert_eq!(
            game.rope_rot_old,
            (-90.0, 0.0),
            "yRotO/xRotO snap: no interpolated turn"
        );
        let player = game.player.as_ref().unwrap();
        assert_eq!(
            (player.yaw, player.head_yaw),
            (-90.0, -90.0),
            "the head turns with it"
        );

        // An id the client doesn't have falls back to the packet's point.
        (game.yaw, game.pitch) = (45.0, 20.0);
        game.rope_rot_old = (45.0, 20.0);
        look_at(game, [3.0, 0.0, 0.0], Some(99));
        assert_eq!(
            (game.yaw, game.pitch),
            (-90.0, 0.0),
            "faces the packet's point"
        );
        assert_eq!(game.rope_rot_old, (-90.0, 0.0));
    });
}
