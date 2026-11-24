//! #3343: a weapon's own `post_attack` kind reaches `enchant_effect` with the
//! real attacker, victim and weapon, for players and armed mobs alike.
#![cfg(test)]
use crate::game::{
    Game, creatures::SpawnSite, native_entity_refs::tests::with_game,
    native_guest_turn_tests::with_guests,
};
use mod_api::{EnchantEffectEvent, Hooks, ModEntry, ServerHost};
use std::sync::{Arc, Mutex};
use sunrise_client::{
    items::ItemStack,
    mobs::{Damage, Mob, MobEvent, finalize},
    motion::PlayerMotion,
};
use sunrise_core::geometry::Vec3;

/// One call: the attacker's and the victim's ids, the kind, whether the
/// weapon was the enchanted iron sword, and whether both handles resolve
/// through the server host.
type Call = (u32, u32, String, bool, bool);
type Heard = Arc<Mutex<Vec<Call>>>;

struct Probe(Heard);
#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn enchant_effect(&self, host: &mut dyn ServerHost, event: &mut EnchantEffectEvent<'_>) {
        let sword = host
            .item_key(&mod_api::Identifier::parse("minecraft:iron_sword").unwrap())
            .unwrap();
        let resolved =
            event.attacker.position(host).is_some() && event.victim.position(host).is_some();
        self.0.lock().unwrap().push((
            event.attacker.id(),
            event.victim.id(),
            event.kind.to_owned(),
            event.weapon.item() == sword,
            resolved,
        ));
    }
}

fn sword() -> ItemStack {
    let mut stack = ItemStack::new("minecraft:iron_sword", 1);
    stack.set_crafting_enchantments(vec![
        ("probe:shulking".into(), 1),
        ("probe:helmet".into(), 1),
    ]);
    stack
}

fn mob(game: &Game<'_>, id: u32, kind: &str, x: f64) -> Mob {
    let kind = game.res.mob_types().get(kind).unwrap().clone();
    Mob::new(id, kind, Vec3::new(x, 64.0, 0.5), 0.0)
}

/// Runs `test` with the probe mod listening.
fn listening(test: impl FnOnce(&Heard)) {
    let heard = Heard::default();
    let mods = vec![ModEntry::new("probe", Box::new(Probe(heard.clone())))];
    mod_platform::testing::with_mods(mods.into(), || test(&heard));
}

/// Adds to `game`'s world enchantments `probe:shulking`, a main-hand
/// enchantment with a `probe:levitate` attacker-to-victim entry and a
/// `probe:player_attack` one that needs a `#minecraft:is_player_attack` hit,
/// and `probe:helmet`, a head-only one with a `probe:levitate` entry that a
/// weapon never applies.
fn install_probe(game: &mut Game<'_>) {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("datapacks/probe");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(
        pack.join("pack.mcmeta"),
        r#"{"pack":{"description":"Probe fixture"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("datapacks.json"),
        r#"{"enabled":["file/probe"],"disabled":[]}"#,
    )
    .unwrap();
    game.world_root = root.path().to_path_buf();
    let packs = game.reload_datapacks(false, &[]).unwrap();
    let resources = game.options.resources.clone();
    game.world_tables = game
        .stage_world_tables(&packs, false, &resources)
        .unwrap()
        .tables;
    let definition = |slot: &str| {
        serde_json::json!({
            "description": "Probe", "supported_items": "minecraft:iron_sword",
            "weight": 1, "max_level": 1, "anvil_cost": 1,
            "min_cost": {"base": 1, "per_level_above_first": 0},
            "max_cost": {"base": 1, "per_level_above_first": 0},
            "slots": [slot],
            "effects": {"minecraft:post_attack": [
                {"enchanted": "attacker", "affected": "victim",
                    "effect": {"type": "probe:levitate", "ticks": 5}},
                {"enchanted": "attacker", "affected": "victim",
                    "effect": {"type": "probe:player_attack"},
                    "requirements": {"condition": "minecraft:damage_source_properties",
                        "predicate": {"tags": [{"id": "minecraft:is_player_attack", "expected": true}]}}}
            ]}
        })
    };
    let (shulking, helmet) = (definition("mainhand"), definition("head"));
    let stations = &mut game.world_tables.as_mut().unwrap().stations;
    // Built on the loaded table, so the damage type tags stay.
    let table = stations
        .enchantments
        .with_entries([("probe:shulking", &shulking), ("probe:helmet", &helmet)])
        .unwrap()
        .unwrap();
    stations.enchantments = Arc::new(table);
}

/// A serving game with the probe installed whose player (entity 7) holds the
/// enchanted iron sword.
fn with_probe(test: impl FnOnce(&mut Game<'_>, &Heard)) {
    listening(|heard| {
        with_game(|game| {
            install_probe(game);
            let mut body = PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap();
            body.entity_id = 7;
            game.player = Some(body);
            game.creative = false;
            game.inventory[game.selected] = Some(sword());
            test(game, heard);
        });
    });
}

/// Two loaded survival guests (12 and 13) with the probe installed; guest 12
/// holds the enchanted iron sword.
fn with_probe_guests(test: impl FnOnce(&mut Game<'_>, &Heard)) {
    listening(|heard| {
        with_guests(|game| {
            install_probe(game);
            for conn in [12, 13] {
                game.lan
                    .as_mut()
                    .unwrap()
                    .hub
                    .update_guest(conn, |guest| {
                        guest.loaded = true;
                        guest.load_timeout = 0;
                        if conn == 12 {
                            guest.inventory[guest.selected] = Some(sword());
                        }
                    })
                    .unwrap();
                game.as_guest(conn, |g| {
                    use sunrise_client::command::args::GameMode;
                    g.set_game_mode(GameMode::Creative);
                    g.set_game_mode(GameMode::Survival);
                })
                .unwrap();
            }
            test(game, heard);
        });
    });
}

fn heard(game_heard: &Heard) -> Vec<Call> {
    std::mem::take(&mut *game_heard.lock().unwrap())
}

/// The calls one landed hit makes: `probe:levitate`, then `probe:player_attack`
/// when the hit's damage type is in `#minecraft:is_player_attack`. The
/// head-only `probe:helmet` on the same sword never adds one. `resolved`:
/// both handles resolve through the server host, which the bare local body
/// of [`with_probe`] (no server player) does not.
fn hit(attacker: u32, victim: u32, player_attack: bool, resolved: bool) -> Vec<Call> {
    let call = |kind: &str| (attacker, victim, kind.to_owned(), true, resolved);
    let mut calls = vec![call("probe:levitate")];
    if player_attack {
        calls.push(call("probe:player_attack"));
    }
    calls
}

#[test]
#[ignore = "daily"]
fn a_players_hit_on_a_mob_tells_the_player_the_mob_and_the_sword() {
    with_probe(|game, calls| {
        game.mobs.push(mob(game, 21, "minecraft:cow", 1.5));
        game.post_attack(0, "minecraft:player_attack");
        assert_eq!(heard(calls), hit(7, 21, true, false));
    });
}

/// A guest's blow through the attack packet's entry point, on another guest.
#[test]
#[ignore = "daily"]
fn a_guests_hit_on_a_guest_tells_both_through_the_server() {
    with_probe_guests(|game, calls| {
        let gi = game.guest_index(12).unwrap();
        assert!(game.guest_attack(12, gi, 13).unwrap());
        assert_eq!(heard(calls), hit(12, 13, true, true));
    });
}

/// A sword's sweep tells the guest it swept as well as the mob it hit.
#[test]
#[ignore = "daily"]
fn a_guests_sweep_tells_each_swept_guest() {
    with_probe_guests(|game, calls| {
        let zombie = game
            .spawn_mob("minecraft:zombie", [3.5, 64.0, 5.5], true, SpawnSite::Raw)
            .unwrap();
        // `isSweepAttack`: a full charge, standing on the ground.
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(12, |guest| {
                guest.on_ground = true;
            })
            .unwrap();
        game.as_guest(12, |g| g.attack_ticks = 20).unwrap();
        let gi = game.guest_index(12).unwrap();
        assert!(game.guest_attack(12, gi, zombie as i32).unwrap());
        game.flush_sweeps();
        let mut expected = hit(12, zombie, true, true);
        expected.extend(hit(12, 13, true, true));
        assert_eq!(heard(calls), expected);
    });
}

#[test]
#[ignore = "daily"]
fn an_armed_mobs_hit_on_a_mob_tells_the_wielder() {
    with_probe(|game, calls| {
        let mut zombie = mob(game, 41, "minecraft:zombie", 1.5);
        finalize::set_item_stack(&mut zombie, finalize::MAINHAND, sword());
        game.mobs.push(zombie);
        // A bare-handed zombie has no weapon to tell of.
        game.mobs.push(mob(game, 42, "minecraft:zombie", 3.5));
        game.mobs.push(mob(game, 21, "minecraft:cow", 2.5));
        game.mobs.push(mob(game, 22, "minecraft:cow", 2.5));
        for (attacker, target) in [(41, 21), (42, 22)] {
            game.handle_mob_events(vec![MobEvent::AttackMob {
                target,
                amount: 1.0,
                source: Damage::mob(attacker, [1.5, 64.0, 0.5]),
                remember_target: false,
            }])
            .unwrap();
        }
        // `mob_attack` is no player attack.
        assert_eq!(heard(calls), hit(41, 21, false, true));
    });
}

#[test]
#[ignore = "daily"]
fn an_armed_mobs_hit_on_a_player_tells_the_wielder_and_the_player() {
    with_probe(|game, calls| {
        let mut zombie = mob(game, 41, "minecraft:zombie", 1.5);
        finalize::set_item_stack(&mut zombie, finalize::MAINHAND, sword());
        game.mobs.push(zombie);
        game.handle_mob_events(vec![MobEvent::AttackPlayer(
            1.0,
            [1.5, 64.0, 0.5],
            Some(41),
        )])
        .unwrap();
        assert_eq!(heard(calls), hit(41, 7, false, false));
    });
}

/// A mob's spear stab on a player tells the wielder once it lands; the same
/// stab again, refused by the player's damage cooldown, tells nothing.
#[test]
#[ignore = "daily"]
fn an_armed_mobs_stab_on_a_player_tells_the_wielder_only_when_it_lands() {
    with_probe(|game, calls| {
        let mut zombie = mob(game, 41, "minecraft:zombie", 1.5);
        finalize::set_item_stack(&mut zombie, finalize::MAINHAND, sword());
        game.mobs.push(zombie);
        let stab = || MobEvent::DamagePlayer {
            amount: 1.0,
            from: [1.5, 64.0, 0.5],
            kind: "minecraft:spear",
            wielder: Some(41),
        };
        game.handle_mob_events(vec![stab()]).unwrap();
        // `minecraft:spear` is in `#minecraft:is_player_attack`.
        assert_eq!(heard(calls), hit(41, 7, true, false));
        game.handle_mob_events(vec![stab()]).unwrap();
        assert_eq!(heard(calls), []);
    });
}
