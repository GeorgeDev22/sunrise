//! A native mod's general rows as a Sunrise client sees them (#792, stage 5): the server's
//! `sunrise:general_entity` spawn adds one, the vanilla packets keyed by its network id move and
//! remove it as they do any tracked entity, and its declared fields are its synced attachments
//! (`Remote::attachments`). The server sends every row its first attachment snapshot (empty or
//! not) after the spawn, in another payload: a row is shown only once both are here, so it is
//! never seen without its initial fields. No mod code runs here: a row's behaviour is the
//! server's.
use super::{Game, Lerp, Remote};
use std::collections::HashMap;
use sunrise_protocol::general_entity::{self, Message};

/// One general row the server shows the window.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::game) struct RemoteGeneral {
    /// The client's frozen catalog slot for the row's type.
    pub type_slot: usize,
    pub uuid: u128,
    pub position: [f64; 3],
    /// Where it was when the last client tick began, for a frame's partial tick.
    pub previous: [f64; 3],
    /// Yaw, then pitch, in degrees.
    pub rotation: [f32; 2],
    pub previous_rotation: [f32; 2],
    /// Client ticks since the spawn.
    pub age: u32,
    /// Its first attachment snapshot has arrived too: the row is shown, with its fields.
    pub shown: bool,
}

/// The rows the server shows the window, by network id, and the warnings already given.
#[derive(Default)]
pub(in crate::game) struct RemoteGenerals {
    pub rows: HashMap<i32, RemoteGeneral>,
    /// Type keys this client's catalog does not hold, each warned about once.
    unknown: std::collections::HashSet<String>,
    /// Whether a spawn for an id already held was refused before, warned about once.
    duplicate: bool,
}

impl RemoteGenerals {
    /// Forget every row: a world change or a respawn into another dimension.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// A whole attachment snapshot for entity `id` arrived: a row waiting for it is shown.
    pub fn snapshot_arrived(&mut self, id: i32) {
        if let Some(row) = self.rows.get_mut(&id) {
            row.shown = true;
        }
    }
}

impl Game<'_> {
    /// A `sunrise:general_entity` payload the server sent the window.
    pub(super) fn remote_general_entity(&mut self, data: &[u8]) {
        let message = match general_entity::decode(data) {
            Ok(message) => message,
            Err(e) => return log::debug!("Sunrise: a bad general entity spawn: {e}"),
        };
        let Message::Spawn {
            id,
            uuid,
            type_key,
            position,
            rotation,
            velocity: _,
        } = message;
        // Both sides froze the same declarations (general types come from both-side
        // packages): a key this client lacks comes from a server that does not match it.
        let Some(type_slot) = crate::game::general::general_type_catalog().slot(&type_key) else {
            if let Some(r) = self.remote.as_mut()
                && r.general.unknown.insert(type_key.clone())
            {
                log::warn!(
                    "Sunrise: the server sent general entity type {type_key}, not installed here"
                );
            }
            return;
        };
        let Some(r) = self.remote.as_mut() else {
            return;
        };
        // The snapshot may come first: a removal drops what the client held of an id, so a
        // copy held now is this row's.
        let shown = r
            .attachments
            .contains_key(&sunrise_mod_runtime::attachments::Holder::Entity(id as u32));
        if r.general.rows.contains_key(&id) {
            if !std::mem::replace(&mut r.general.duplicate, true) {
                log::warn!("Sunrise: the server sent general entity {id} twice; the first stays");
            }
            return;
        }
        r.general.rows.insert(
            id,
            RemoteGeneral {
                type_slot,
                uuid,
                position,
                previous: position,
                rotation,
                previous_rotation: rotation,
                age: 0,
                shown,
            },
        );
        self.persist
            .uuids
            .insert(id as u32, sunrise_client::lan::playerdata::uuid_ints(uuid));
        // Tracked: the id-keyed move, position sync, teleport and removal handlers apply.
        self.spawn_entity(
            id,
            &type_key,
            position,
            [rotation[0], rotation[1], rotation[0]],
        );
        if let Some(r) = self.remote.as_mut() {
            r.lerps.remove(&id);
        }
    }

    /// The server removed general row `id` from the window.
    pub(super) fn forget_remote_general(r: &mut Remote, id: i32) {
        r.general.rows.remove(&id);
    }
}

/// One client tick of the rows: `setOldPosAndRot`, then a step of the non-living
/// `InterpolationHandler` (`Entity.moveOrInterpolateTo`, 3 steps) toward the server's last move.
pub(super) fn tick_remote_general(
    rows: &mut HashMap<i32, RemoteGeneral>,
    lerps: &mut HashMap<i32, Lerp>,
) {
    for (id, row) in rows.iter_mut() {
        row.previous = row.position;
        row.previous_rotation = row.rotation;
        row.age = row.age.saturating_add(1);
        let Some(l) = lerps.get_mut(id).filter(|l| l.steps > 0) else {
            continue;
        };
        let n = f64::from(l.steps);
        for k in 0..3 {
            row.position[k] += (l.target[k] - row.position[k]) / n;
        }
        if let Some((yaw, pitch)) = l.rotation {
            row.rotation[0] =
                crate::game::remote_objects::lerp_yaw_step(row.rotation[0], yaw, l.steps);
            row.rotation[1] += (pitch - row.rotation[1]) / l.steps as f32;
        }
        l.steps -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_protocol::packets::play::{self, Clientbound as C};

    fn row(at: [f64; 3]) -> RemoteGeneral {
        RemoteGeneral {
            type_slot: 0,
            uuid: 1,
            position: at,
            previous: at,
            rotation: [0.0, 0.0],
            previous_rotation: [0.0, 0.0],
            age: 0,
            shown: true,
        }
    }

    #[test]
    fn a_move_is_reached_in_three_ticks_and_each_tick_keeps_the_one_before() {
        let mut rows = HashMap::from([(5, row([0.0, 70.0, 0.0]))]);
        let mut lerps = HashMap::from([(
            5,
            Lerp {
                target: [3.0, 70.0, 0.0],
                rotation: Some((90.0, 30.0)),
                steps: 3,
            },
        )]);
        let mut seen = Vec::new();
        for _ in 0..4 {
            tick_remote_general(&mut rows, &mut lerps);
            let r = &rows[&5];
            seen.push((r.previous[0], r.position[0], r.rotation, r.age));
        }
        assert_eq!(seen[0], (0.0, 1.0, [30.0, 10.0], 1));
        assert_eq!(seen[1], (1.0, 2.0, [60.0, 20.0], 2));
        assert_eq!(seen[2], (2.0, 3.0, [90.0, 30.0], 3));
        // Reached: it stays, and only ages.
        assert_eq!(seen[3], (3.0, 3.0, [90.0, 30.0], 4));
    }
    struct Declare;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Declare {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.declare_general_entity(
                "missile",
                mod_api::GeneralEntityCategory::Misc,
                0.5,
                0.5,
                mod_api::GeneralSavePolicy::Save,
                true,
                8,
                1,
                1,
                &[],
            )
            .unwrap();
        }
    }

    fn spawn(id: i32, key: &str, at: [f64; 3]) -> C {
        C::CustomPayload(sunrise_protocol::packets::configuration::CustomPayload {
            channel: general_entity::CHANNEL.into(),
            data: general_entity::encode(&Message::Spawn {
                id,
                uuid: 0x77,
                type_key: key.into(),
                position: at,
                rotation: [0.0, 0.0],
                velocity: [0.0; 3],
            })
            .unwrap(),
        })
    }

    /// The server's whole snapshot of entity `id`'s synced attachments: `value` in
    /// `mymod:fuel`, or nothing synced.
    fn snapshot(id: u32, value: Option<i32>) -> C {
        use sunrise_core::nbt::Tag;
        use sunrise_mod_runtime::attachments::{AttachmentSync, Holder};
        let changes = value.map_or_else(Vec::new, |n| {
            vec![AttachmentSync {
                namespace: "mymod".into(),
                name: "fuel".into(),
                value: Some([("n".into(), Tag::Int(n))].into()),
            }]
        });
        C::CustomPayload(sunrise_protocol::packets::configuration::CustomPayload {
            channel: sunrise_protocol::attachment_sync::CHANNEL.into(),
            data: sunrise_protocol::attachment_sync::encode(
                &sunrise_protocol::attachment_sync::Message {
                    holder: Holder::Entity(id),
                    replace: true,
                    changes,
                },
            )
            .unwrap(),
        })
    }

    /// The ids of the rows shown on the window.
    fn shown(game: &Game<'_>) -> Vec<i32> {
        let rows = &game.remote.as_ref().unwrap().general.rows;
        let mut ids: Vec<i32> = rows
            .iter()
            .filter(|(_, row)| row.shown)
            .map(|(&id, _)| id)
            .collect();
        ids.sort_unstable();
        ids
    }

    fn rows(game: &Game<'_>) -> HashMap<i32, RemoteGeneral> {
        game.remote.as_ref().unwrap().general.rows.clone()
    }

    /// The window's side of a general row: a spawn adds it in the client's catalog slot, an
    /// unknown key and a second spawn of one id change nothing; it is shown only once its first
    /// snapshot is here too, whichever comes first, with that snapshot's value; a position sync
    /// is reached over three ticks with nothing of the mod's run here, and a removal drops it
    /// with its synced attachments.
    #[test]
    #[ignore = "daily"]
    fn the_window_keeps_moves_and_drops_the_rows_it_is_shown() {
        mod_platform::__private::initialize().unwrap();
        let _phase =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        let declare = || vec![mod_api::ModEntry::new("mymod", Box::new(Declare))];
        mod_platform::testing::with_mods(declare().into(), || {
            mod_platform::__private::part_tests::install_parts_fixture(&declare()).unwrap();
            crate::game::item_entities::thrown_items_tests::with_remote_game(|game| {
                game.handle_packet(spawn(5, "mymod:missile", [0.5, 70.0, 0.5]))
                    .unwrap();
                game.handle_packet(spawn(6, "other:unknown", [0.5, 70.0, 0.5]))
                    .unwrap();
                game.handle_packet(spawn(5, "mymod:missile", [9.5, 70.0, 0.5]))
                    .unwrap();
                let held = rows(game);
                assert_eq!(held.keys().collect::<Vec<_>>(), [&5]);
                assert_eq!(held[&5].position, [0.5, 70.0, 0.5]);
                assert_eq!(held[&5].uuid, 0x77);
                // First pairing: the spawn alone shows nothing; with its snapshot, the row and
                // its value are there together.
                assert!(shown(game).is_empty());
                game.handle_packet(snapshot(5, Some(9))).unwrap();
                assert_eq!(shown(game), [5]);
                let fuel = |game: &Game<'_>, id: u32| {
                    let held = game.remote.as_ref().unwrap();
                    let map = held
                        .attachments
                        .get(&sunrise_mod_runtime::attachments::Holder::Entity(id))?;
                    // A synced value arrives raw, as `{value: <compound>}` (`apply_sync`).
                    let saved = map.entry("mymod", "fuel").ok()??.saved()?;
                    let Some(sunrise_core::nbt::Tag::Compound(value)) = saved.get(&"value".into())
                    else {
                        return None;
                    };
                    match value.get(&"n".into()) {
                        Some(sunrise_core::nbt::Tag::Int(n)) => Some(*n),
                        _ => None,
                    }
                };
                assert_eq!(fuel(game, 5), Some(9));
                // The snapshot first, an empty one: the spawn that follows is shown at once.
                game.handle_packet(snapshot(8, None)).unwrap();
                assert_eq!(shown(game), [5]);
                game.handle_packet(spawn(8, "mymod:missile", [0.5, 70.0, 0.5]))
                    .unwrap();
                assert_eq!(shown(game), [5, 8]);
                game.handle_packet(C::EntityPositionSync(play::EntityPositionSync {
                    id: 5,
                    path: play::PositionPath::Linear([3.5, 70.0, 0.5]),
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: false,
                }))
                .unwrap();
                let mut xs = Vec::new();
                for _ in 0..3 {
                    game.tick_remote_entities(true);
                    xs.push(rows(game)[&5].position[0]);
                }
                assert_eq!(xs, [1.5, 2.5, 3.5]);
                assert_eq!(rows(game)[&5].previous[0], 2.5);
                // A client holds rows only here: none is a resident row, ticked by mod code.
                assert!(game.resident_entities.general.rows.is_empty());
                let holder = sunrise_mod_runtime::attachments::Holder::Entity(5);
                game.remote
                    .as_mut()
                    .unwrap()
                    .attachments
                    .insert(holder, Default::default());
                game.handle_packet(C::RemoveEntities(play::RemoveEntities { ids: vec![5, 8] }))
                    .unwrap();
                assert!(rows(game).is_empty());
                assert!(
                    !game
                        .remote
                        .as_ref()
                        .unwrap()
                        .attachments
                        .contains_key(&holder)
                );
            })
        });
    }
}
