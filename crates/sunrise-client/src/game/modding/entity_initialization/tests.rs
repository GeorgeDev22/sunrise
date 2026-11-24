use super::*;
use crate::game::{
    Dim,
    native_entity_refs::tests::{with_client_game, with_game},
};
use mod_api::{AttachmentDescriptor, AttachmentKey, EntityConstructingEvent, Hooks, HostSide};
use std::sync::{Arc, Mutex};
use sunrise_core::nbt::Tag;

type Calls = Arc<Mutex<Vec<(bool, Holder)>>>;
type KeySlot = Arc<Mutex<Option<AttachmentKey<i32>>>>;

struct Listener {
    calls: Calls,
    value: KeySlot,
}

#[mod_api::__subscriptions]
impl Hooks for Listener {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        let key = r
            .register_attachment_type(AttachmentDescriptor::new("constructing:value", || 0i32))
            .unwrap();
        *self.value.lock().unwrap() = Some(key);
    }
    fn entity_constructing(&self, host: HostSide<'_>, event: &mut EntityConstructingEvent) {
        self.calls
            .lock()
            .unwrap()
            .push((matches!(host, HostSide::Client(_)), event.holder));
        let value = self
            .value
            .lock()
            .unwrap()
            .clone()
            .expect("Listener registered");
        // Both a read and a write inside dispatch must reach the canonical map
        // without recursively constructing the same entity.
        match host {
            HostSide::Server(host) => {
                let _ = value.get(host, event.holder, Clone::clone).unwrap();
                value.set(host, event.holder, 41).unwrap();
            }
            HostSide::Client(host) => {
                let _ = value.get(host, event.holder, Clone::clone).unwrap();
                value.set(host, event.holder, 41).unwrap();
            }
        }
    }
}

fn observe_with_key(test: impl FnOnce(Calls, KeySlot)) {
    let calls = Calls::default();
    let value = KeySlot::default();
    let mods = vec![mod_api::ModEntry::new(
        "constructing",
        Box::new(Listener {
            calls: calls.clone(),
            value: value.clone(),
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || test(calls, value));
}

fn tag(kind: &str, uuid: i32) -> Tag {
    Tag::compound([
        ("id", Tag::String(kind.into())),
        ("UUID", Tag::IntArray(vec![uuid, 2, 3, 4])),
        (
            "Pos",
            Tag::List(
                [1.0f64, 70.0, 3.0]
                    .map(|v| Tag::Double(v.to_bits()))
                    .to_vec(),
            ),
        ),
    ])
}

#[test]
#[ignore = "daily"]
fn spawn_summon_and_disk_load_initialize_before_first_attachment_read() {
    observe_with_key(|calls, value_slot| {
        with_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            game.options.dedicated = Some(Vec::new());
            let spawn = game
                .spawn_mob_spawned(
                    "cow",
                    [1.0, 70.0, 3.0],
                    crate::game::creatures::SpawnSite::Plain,
                )
                .unwrap();
            assert_eq!(
                value_key.get(game, Holder::Entity(spawn), Clone::clone),
                Ok(41)
            );
            game.spawn_entity_from_nbt(tag("minecraft:pig", 100));
            let summoned = game
                .persist
                .uuids
                .iter()
                .find(|(_, uuid)| **uuid == [100, 2, 3, 4])
                .map(|(id, _)| *id)
                .unwrap();
            assert_eq!(
                value_key.get(game, Holder::Entity(summoned), Clone::clone),
                Ok(41)
            );
            for (i, kind) in [
                "minecraft:zombie",
                "minecraft:arrow",
                "minecraft:snowball",
                "minecraft:oak_boat",
                "minecraft:minecart",
                "minecraft:armor_stand",
                "minecraft:marker",
            ]
            .into_iter()
            .enumerate()
            {
                let uuid = 101 + i as i32;
                game.world
                    .loaded_entities
                    .push(((0, 0), vec![tag(kind, uuid).into()]));
                game.spawn_loaded_entities();
                let id = *game
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, value)| **value == [uuid, 2, 3, 4])
                    .unwrap()
                    .0;
                let expected = Ok(41);
                assert_eq!(
                    value_key.get(game, Holder::Entity(id), Clone::clone),
                    expected,
                    "{kind}"
                );
                assert_eq!(
                    calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(_, h)| *h == Holder::Entity(id))
                        .count(),
                    1,
                    "{kind}"
                );
            }
            let count = calls.lock().unwrap().len();
            game.native_joins_flush(false, 0);
            game.native_joins_flush(true, 0);
            game.entity_chunks(&HashSet::from([(0, 0)]), false);
            game.remember_uuid(summoned, Some([100, 2, 3, 4]), &tag("minecraft:pig", 100));
            assert_eq!(calls.lock().unwrap().len(), count);
            assert!(calls.lock().unwrap().iter().all(|(client, _)| !client));
        })
    });
}

#[test]
#[ignore = "daily"]
fn borrowed_spawn_first_read_and_dimension_transfer_keep_one_initialization() {
    observe_with_key(|calls, value_slot| {
        with_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            game.options.dedicated = Some(Vec::new());
            // A borrowed simulation pass can insert without running Game's spawn helper.
            let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
            let id = game.take_entity_id();
            game.mobs.push(sunrise_client::mobs::Mob::new(
                id,
                kind,
                sunrise_core::geometry::Vec3::new(1.0, 70.0, 3.0),
                0.0,
            ));
            assert_eq!(
                value_key.get(game, Holder::Entity(id), Clone::clone),
                Ok(41)
            );
            game.native_joins_flush(false, 0);
            assert_eq!(calls.lock().unwrap().len(), 1);
            let uuid = game.uuid_for(id);
            let saved = game
                .entity_chunks(&HashSet::from([(0, 0)]), false)
                .into_iter()
                .flat_map(|(_, tags)| tags)
                .find(|t| sunrise_client::entity_nbt::uuid(t) == Some(uuid))
                .unwrap();
            // Portals move a live entity using its saved representation, retaining its map.
            let state = game.take_transferred_entity_state(id);
            game.mobs.retain(|mob| mob.id != id);
            game.detach_entity(id);
            game.enter_level(Dim::Nether).unwrap();
            let moved = game.load_mob_transferred(saved, None, Some(state)).unwrap();
            assert_eq!(game.persist.uuid_of(moved), Some(uuid));
            assert_eq!(
                value_key.get(game, Holder::Entity(moved), Clone::clone),
                Ok(41)
            );
            assert_eq!(calls.lock().unwrap().len(), 1);
            game.forget_entity(moved);
            game.mobs.retain(|mob| mob.id != moved);
            let replacement = game.load_mob(tag("minecraft:cow", uuid[0]), None).unwrap();
            assert_eq!(
                value_key.get(game, Holder::Entity(replacement), Clone::clone),
                Ok(41)
            );
            assert_eq!(calls.lock().unwrap().len(), 2);
            // Non-Mob transfers must retain values that cannot be serialized at all.
            for (n, kind) in [
                "minecraft:item",
                "minecraft:oak_boat",
                "minecraft:minecart",
                "minecraft:arrow",
                "minecraft:armor_stand",
            ]
            .into_iter()
            .enumerate()
            {
                let uuid = [300 + n as i32, 2, 3, 4];
                let mut data = tag(kind, uuid[0]);
                if kind == "minecraft:item"
                    && let Tag::Compound(c) = &mut data
                {
                    c.insert(
                        "Item".into(),
                        Tag::compound([
                            ("id", Tag::String("minecraft:stone".into())),
                            ("count", Tag::Int(1)),
                        ]),
                    );
                }
                game.load_listed(kind, Some(uuid), None, data.clone())
                    .unwrap();
                let source = *game
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, u)| **u == uuid)
                    .unwrap()
                    .0;
                value_key.set(game, Holder::Entity(source), 73).unwrap();
                let before = calls.lock().unwrap().len();
                let state = game.take_transferred_entity_state(source);
                let entity = game.entity(source).unwrap();
                assert!(game.entity_discard(entity));
                game.native_removals_flush();
                game.forget_entity(source);
                let destination = if game.dim == Dim::Nether {
                    Dim::Overworld
                } else {
                    Dim::Nether
                };
                game.enter_level(destination).unwrap();
                game.load_listed_transferred(kind, Some(uuid), None, data, state)
                    .unwrap();
                let arrived = *game
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, u)| **u == uuid)
                    .unwrap()
                    .0;
                assert_ne!(source, arrived);
                assert_eq!(
                    value_key.get(game, Holder::Entity(arrived), Clone::clone),
                    Ok(73),
                    "{kind}"
                );
                assert_eq!(
                    calls.lock().unwrap().len(),
                    before,
                    "{kind} transferred, not constructed"
                );
            }
            // Constructor ownership is pruned by object identity before UUID cleanup.
            // Equal UUIDs must not make an unreachable object's marker erase a survivor;
            // player holders and objects belonging to parked levels stay untouched.
            let removed = Holder::Entity(900_001);
            let retained = Holder::Entity(900_002);
            let parked = Holder::Entity(900_003);
            let player = Holder::Player(900_001);
            game.entity_initialization
                .initialized
                .extend([removed, retained, parked, player]);
            game.persist.uuids.insert(900_001, [901, 2, 3, 4]);
            game.persist.uuids.insert(900_002, [901, 2, 3, 4]);
            game.prune_entity_initializations(&HashSet::from([900_002]));
            assert!(!game.entity_initialization.initialized.contains(&removed));
            for holder in [retained, parked, player] {
                assert!(game.entity_initialization.initialized.contains(&holder));
            }
        })
    });
}

fn spawn_info() -> sunrise_protocol::packets::play::SpawnInfo {
    sunrise_protocol::packets::play::SpawnInfo {
        dimension_type: 0,
        dimension: "minecraft:overworld".into(),
        seed: 1,
        game_type: 0,
        previous_game_type: None,
        debug: false,
        flat: false,
        last_death: None,
        portal_cooldown: 0,
        sea_level: 63,
    }
}

#[test]
#[ignore = "daily"]
fn client_login_tracking_and_retracking_initialize_the_client_store_once() {
    use sunrise_protocol::packets::play::{self, Clientbound as C};
    observe_with_key(|calls, value_slot| {
        with_client_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            let (pipe, _server) = sunrise_protocol::connection::Pipe::pair();
            game.connect_local(pipe, "Steve").unwrap();
            game.handle_packet(C::Login(play::Login {
                player_id: 7,
                hardcore: false,
                levels: vec!["minecraft:overworld".into()],
                max_players: 20,
                chunk_radius: 8,
                simulation_distance: 8,
                reduced_debug_info: false,
                show_death_screen: true,
                limited_crafting: false,
                spawn: spawn_info(),
                online_mode: false,
                enforces_secure_chat: false,
            }))
            .unwrap();
            let profile = game.remote.as_ref().unwrap().profile;
            assert_eq!(
                value_key.get(game, Holder::Player(profile), Clone::clone),
                Ok(41)
            );
            game.handle_packet(C::PlayerInfoUpdate(play::PlayerInfoUpdate {
                actions: 1,
                entries: vec![play::PlayerInfo {
                    uuid: 22,
                    name: Some("Remote".into()),
                    ..Default::default()
                }],
            }))
            .unwrap();
            let registry = game
                .remote
                .as_ref()
                .unwrap()
                .builtin
                .get("minecraft:entity_type")
                .unwrap()
                .clone();
            for (id, kind) in [
                (20, "minecraft:pig"),
                (21, "minecraft:arrow"),
                (22, "minecraft:player"),
            ] {
                let packet = || {
                    C::AddEntity(play::AddEntity {
                        id,
                        uuid: id as u128,
                        kind: registry.id(kind).unwrap(),
                        position: [1.0, 70.0, 3.0],
                        velocity: [0.0; 3],
                        pitch: 0,
                        yaw: 0,
                        head_yaw: 0,
                        data: 0,
                    })
                };
                game.handle_packet(packet()).unwrap();
                let holder = if kind == "minecraft:player" {
                    Holder::Player(id as u128)
                } else {
                    Holder::Entity(id as u32)
                };
                assert_eq!(value_key.get(game, holder, Clone::clone), Ok(41));
                game.handle_packet(packet()).unwrap();
                assert_eq!(
                    calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(_, h)| *h == holder)
                        .count(),
                    1
                );
            }
            // Removing and retracking creates a new object even for the same ID/UUID.
            game.handle_packet(C::RemoveEntities(play::RemoveEntities {
                ids: vec![20, 21, 22],
            }))
            .unwrap();
            for (id, kind) in [
                (20, "minecraft:pig"),
                (21, "minecraft:arrow"),
                (22, "minecraft:player"),
            ] {
                game.handle_packet(C::AddEntity(play::AddEntity {
                    id,
                    uuid: id as u128,
                    kind: registry.id(kind).unwrap(),
                    position: [1.0, 70.0, 3.0],
                    velocity: [0.0; 3],
                    pitch: 0,
                    yaw: 0,
                    head_yaw: 0,
                    data: 0,
                }))
                .unwrap();
                let holder = if kind == "minecraft:player" {
                    Holder::Player(id as u128)
                } else {
                    Holder::Entity(id as u32)
                };
                assert_eq!(
                    calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(_, h)| *h == holder)
                        .count(),
                    2
                );
            }
            assert!(calls.lock().unwrap().iter().all(|(client, _)| *client));
            assert_eq!(calls.lock().unwrap().len(), 7);
        })
    });
}

#[test]
#[ignore = "daily"]
fn server_players_initialize_on_join_and_new_respawn_but_not_rebind() {
    observe_with_key(|calls, value_slot| {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            game.options.dedicated = Some(Vec::new());
            game.world_root = game.save.root.clone();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .guests
                .update_all(|guest| guest.placed = false);
            game.lan_guest_joined(12, &[], [0.5, 64.0, 0.5], None);
            assert_eq!(
                value_key.get(game, Holder::Player(12), Clone::clone),
                Ok(41)
            );
            game.bind_player_attachments(12, false);
            game.initialize_server_player(12, 12);
            assert_eq!(
                calls.lock().unwrap().as_slice(),
                &[(false, Holder::Player(12))]
            );
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .guests
                .update(0, |guest| guest.dead = true);
            game.guest_respawn(12);
            assert_eq!(
                value_key.get(game, Holder::Player(12), Clone::clone),
                Ok(41)
            );
            assert_eq!(
                calls.lock().unwrap().as_slice(),
                &[(false, Holder::Player(12)), (false, Holder::Player(12))]
            );
        })
    });
}

fn encode(n: &i32) -> Result<sunrise_core::nbt::Compound, mod_api::AttachmentError> {
    Ok([("value".into(), Tag::Int(*n))].into())
}
fn decode(c: &sunrise_core::nbt::Compound) -> Result<i32, mod_api::AttachmentError> {
    match c.get(&"value".into()) {
        Some(Tag::Int(n)) => Ok(*n),
        _ => Err(mod_api::AttachmentError::Decode),
    }
}
const SAVED_RAW: sunrise_mod_runtime::attachments::AttachmentType<i32> =
    sunrise_mod_runtime::attachments::AttachmentType {
        namespace: "constructing",
        name: "saved",
        default: || 0,
        codec: Some(mod_api::AttachmentCodec { encode, decode }),
        copy_on_death: false,
        sync_codec: None,
    };
type Kinds = Arc<Mutex<Vec<(Holder, String)>>>;

struct Saving {
    kinds: Kinds,
    value: KeySlot,
    saved: KeySlot,
}

#[mod_api::__subscriptions]
impl Hooks for Saving {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        let value = r
            .register_attachment_type(AttachmentDescriptor::new("constructing:value", || 0i32))
            .unwrap();
        let saved = r
            .register_attachment_type(
                AttachmentDescriptor::new("constructing:saved", || 0i32)
                    .serialize(mod_api::AttachmentCodec { encode, decode }),
            )
            .unwrap();
        *self.value.lock().unwrap() = Some(value);
        *self.saved.lock().unwrap() = Some(saved);
    }
    fn entity_constructing(&self, host: HostSide<'_>, event: &mut EntityConstructingEvent) {
        self.kinds
            .lock()
            .unwrap()
            .push((event.holder, event.kind.to_owned()));
        if let HostSide::Server(host) = host {
            let saved = self
                .saved
                .lock()
                .unwrap()
                .clone()
                .expect("Saving registered");
            let value = self
                .value
                .lock()
                .unwrap()
                .clone()
                .expect("Saving registered");
            saved.set(host, event.holder, 41).unwrap();
            value.set(host, event.holder, 41).unwrap();
        }
    }
}

#[test]
#[ignore = "daily"]
fn a_loaded_entity_reads_its_saved_attachments_over_the_listeners_and_is_named_by_type() {
    let kinds = Kinds::default();
    let value_slot = KeySlot::default();
    let saved_slot = KeySlot::default();
    let mods = vec![mod_api::ModEntry::new(
        "constructing",
        Box::new(Saving {
            kinds: kinds.clone(),
            value: value_slot.clone(),
            saved: saved_slot.clone(),
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Saving registered");
            let saved_key = saved_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Saving registered");
            game.options.dedicated = Some(Vec::new());
            let mut saved = sunrise_mod_runtime::attachments::Attachments::default();
            SAVED_RAW.set(&mut saved, Holder::Level, 7).unwrap();
            let mut pig = tag("minecraft:pig", 200);
            if let Tag::Compound(c) = &mut pig {
                c.insert(
                    sunrise_mod_runtime::attachments::ATTACHMENTS_KEY.into(),
                    Tag::Compound(saved.try_to_nbt().unwrap()),
                );
            }
            game.world.loaded_entities.push(((0, 0), vec![pig.into()]));
            game.spawn_loaded_entities();
            let id = *game
                .persist
                .uuids
                .iter()
                .find(|(_, value)| **value == [200, 2, 3, 4])
                .unwrap()
                .0;
            // `Entity.load` after `EntityConstructing`: the saved value wins, the unsaved one stays.
            assert_eq!(saved_key.get(game, Holder::Entity(id), Clone::clone), Ok(7));
            assert_eq!(
                value_key.get(game, Holder::Entity(id), Clone::clone),
                Ok(41)
            );
            assert_eq!(
                kinds.lock().unwrap().as_slice(),
                &[(Holder::Entity(id), "minecraft:pig".to_owned())]
            );
            // Equal saved UUIDs identify distinct objects, including rejected passengers.
            game.native_level_opened(Dim::Overworld);
            SAVED_RAW.set(&mut saved, Holder::Level, 9).unwrap();
            let mut child = tag("minecraft:pig", 200);
            if let Tag::Compound(c) = &mut child {
                c.insert(
                    sunrise_mod_runtime::attachments::ATTACHMENTS_KEY.into(),
                    Tag::Compound(saved.try_to_nbt().unwrap()),
                );
            }
            let ghost = game.load_mob(child.clone(), Some(id)).unwrap();
            assert!(game.entity(ghost).is_none());
            assert_eq!(
                saved_key.get(game, Holder::Entity(ghost), Clone::clone),
                Err(mod_api::AttachmentError::MissingHolder)
            );
            assert_eq!(
                SAVED_RAW.get(
                    game.entity_attachments.get_mut(&ghost).unwrap(),
                    Holder::Entity(ghost)
                ),
                Ok(9)
            );
            assert_eq!(saved_key.get(game, Holder::Entity(id), Clone::clone), Ok(7));
            assert_eq!(kinds.lock().unwrap().len(), 2);
            game.prune_entity_attachments();
            assert!(game.entity_attachments.contains_key(&ghost));
            use mod_api::{AttachmentQueries, AttachmentQueriesMut};
            let batch = game.query_mut(&saved_key).unwrap();
            assert!(batch.iter().try_next().unwrap().is_some());
            let row = batch.iter().try_next().unwrap().unwrap();
            assert_eq!(
                row.with_value(Default::default(), |value| *value).unwrap(),
                7
            );
            // Serialize by object ID: the child's equal UUID cannot select the root's map.
            crate::game::persist::write_attachments(&mut child, ghost, &game.entity_attachments)
                .unwrap();
            let child_state = game.take_transferred_entity_state(ghost);
            let parent_state = game.take_transferred_entity_state(id);
            game.persist.off_level.remove(&ghost);
            game.mobs.retain(|mob| mob.id != id);
            game.detach_entity(ghost);
            game.detach_entity(id);
            game.enter_level(Dim::Nether).unwrap();
            // Passenger-first destination admission reverses which equal-UUID object is resident.
            let arrived = game
                .load_mob_transferred(child, None, Some(child_state))
                .unwrap();
            let refused = game
                .load_mob_transferred(tag("minecraft:pig", 200), Some(arrived), Some(parent_state))
                .unwrap();
            assert_eq!(
                saved_key.get(game, Holder::Entity(arrived), Clone::clone),
                Ok(9)
            );
            assert!(game.entity(refused).is_none());
            assert_eq!(
                kinds.lock().unwrap().len(),
                2,
                "transfer is not reconstruction"
            );
            let batch = game.query(&saved_key).unwrap();
            let mut rows = batch.iter();
            assert!(rows.try_next().unwrap().is_some());
            assert!(rows.try_next().unwrap().is_none());
            assert_eq!(
                row.set(game, 88),
                Err(mod_api::AttachmentRowError::StaleHolder)
            );
            assert_eq!(
                row.with_value(game.attachment_frame_borrow(), |value| *value)
                    .unwrap(),
                7
            );
            game.persist.off_level.remove(&refused);
            game.forget_entity(refused);
            game.prune_entity_attachments();
            assert_eq!(
                saved_key.get(game, Holder::Entity(arrived), Clone::clone),
                Ok(9)
            );
            let batch = game.query(&saved_key).unwrap();
            let mut rows = batch.iter();
            assert!(rows.try_next().unwrap().is_some());
            assert!(rows.try_next().unwrap().is_none());
            game.forget_entity(arrived);
            game.mobs.retain(|mob| mob.id != arrived);
            let rebuilt = game.load_mob(tag("minecraft:pig", 200), None).unwrap();
            assert_eq!(
                saved_key.get(game, Holder::Entity(rebuilt), Clone::clone),
                Ok(41)
            );
            assert_eq!(
                kinds.lock().unwrap().len(),
                3,
                "unload/reload constructs again"
            );
        })
    });
}

#[test]
#[ignore = "daily"]
fn mob_constructor_first_store_binds_only_the_resident_lifetime() {
    use mod_api::AttachmentQueries;
    observe_with_key(|_, value_slot| {
        with_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            game.options.dedicated = Some(Vec::new());
            game.native_level_opened(Dim::Overworld);
            let id = game
                .spawn_mob_spawned(
                    "cow",
                    [1.0, 70.0, 3.0],
                    crate::game::creatures::SpawnSite::Plain,
                )
                .unwrap();
            assert!(
                game.mobs
                    .iter()
                    .find(|mob| mob.id == id)
                    .unwrap()
                    .attachment_lifetime
                    .is_bound()
            );
            let batch = game.query(&value_key).unwrap();
            let mut rows = batch.iter();
            let row = rows.try_next().unwrap().unwrap();
            assert!(rows.try_next().unwrap().is_none());
            assert_eq!(
                row.with_value(game.attachment_frame_borrow(), |value| *value),
                Ok(41)
            );
            assert_eq!(
                row.holder,
                mod_api::AttachmentHolder::Entity(game.entity(id).unwrap())
            );
        });
    });
}

#[test]
#[ignore = "daily"]
fn marker_constructor_store_reaches_its_real_owned_map() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Store(Arc<AtomicBool>, KeySlot);
    #[mod_api::__subscriptions]
    impl Hooks for Store {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            let value = r
                .register_attachment_type(AttachmentDescriptor::new("constructing:value", || 0i32))
                .unwrap();
            *self.1.lock().unwrap() = Some(value);
        }
        fn entity_constructing(&self, host: HostSide<'_>, event: &mut EntityConstructingEvent) {
            if let HostSide::Server(host) = host {
                let value = self.1.lock().unwrap().clone().expect("Store registered");
                assert_eq!(value.set(host, event.holder, 41), Ok(()));
                self.0.store(true, Ordering::SeqCst);
            }
        }
    }
    let ran = Arc::new(AtomicBool::new(false));
    mod_platform::testing::with_mods(
        vec![mod_api::ModEntry::new(
            "constructing",
            Box::new(Store(ran.clone(), KeySlot::default())),
        )]
        .into(),
        || {
            with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.native_level_opened(Dim::Overworld);
                game.spawn_entity_from_nbt(tag("minecraft:marker", 909));
            });
        },
    );
    assert!(ran.load(Ordering::SeqCst));
}

#[test]
#[ignore = "daily"]
fn cancelled_mob_creation_forgets_constructor_payload_and_identity() {
    struct Cancel;
    #[mod_api::__subscriptions]
    impl Hooks for Cancel {
        fn finalize_spawn(
            &self,
            _: &dyn mod_api::Host,
            event: &mut mod_api::FinalizeSpawnEvent<'_>,
        ) -> mod_api::Flow {
            event.spawn_cancelled = true;
            mod_api::Flow::Continue
        }
    }
    let calls = Calls::default();
    mod_platform::testing::with_mods(
        vec![
            mod_api::ModEntry::new(
                "constructing",
                Box::new(Listener {
                    calls: calls.clone(),
                    value: KeySlot::default(),
                }),
            ),
            mod_api::ModEntry::new("cancel", Box::new(Cancel)),
        ]
        .into(),
        || {
            with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.native_level_opened(Dim::Overworld);
                let before = game.persist.uuids.clone();
                assert!(
                    game.spawn_mob_spawned(
                        "cow",
                        [1.0, 70.0, 3.0],
                        crate::game::creatures::SpawnSite::Finalizing(
                            crate::game::creatures::SpawnReason::Command
                        )
                    )
                    .is_none()
                );
                assert_eq!(calls.lock().unwrap().len(), 1);
                assert_eq!(*game.persist.uuids, before);
                assert!(game.entity_attachments.is_empty());
                assert!(game.entity_initialization.initialized.is_empty());
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn dragon_part_constructor_first_read_indexes_payload_and_same_id_replacement_reconstructs() {
    use mod_api::AttachmentQueries;
    observe_with_key(|calls, value_slot| {
        with_game(|game| {
            let value_key = value_slot
                .lock()
                .unwrap()
                .clone()
                .expect("Listener registered");
            game.options.dedicated = Some(Vec::new());
            game.native_level_opened(Dim::Overworld);
            game.spawn_remote_dragon(420, [1.0, 80.0, 1.0], 0.0);
            let holder = Holder::Entity(421);
            // Only an inspection outside dispatch: the constructor listener
            // performs the store, and there is no subsequent explicit store.
            assert_eq!(
                value_key.get_existing(game, holder, |value| value.cloned()),
                Ok(Some(41))
            );
            assert!(game.dragon.as_mut().unwrap().parts[0].2.is_bound());
            let batch = game.query(&value_key).unwrap();
            let mut rows = batch.iter();
            let row = rows.try_next().unwrap().unwrap();
            assert!(rows.try_next().unwrap().is_none());
            assert_eq!(
                row.with_value(game.attachment_frame_borrow(), |value| *value),
                Ok(41)
            );
            assert_eq!(
                row.holder,
                mod_api::AttachmentHolder::Entity(game.entity(421).unwrap())
            );
            game.prune_entity_attachments();
            game.prune_entity_memory();
            assert!(game.entity_attachments.contains_key(&421));
            assert!(game.entity_initialization.initialized.contains(&holder));
            assert_eq!(
                calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, h)| *h == holder)
                    .count(),
                1
            );

            game.spawn_remote_dragon(420, [1.0, 80.0, 1.0], 0.0);
            assert!(!game.entity_attachments.contains_key(&421));
            assert!(!game.entity_initialization.initialized.contains(&holder));
            assert!(
                game.query(&value_key)
                    .unwrap()
                    .iter()
                    .try_next()
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                value_key.get_existing(game, holder, |value| value.cloned()),
                Ok(Some(41))
            );
            assert_eq!(
                calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, h)| *h == holder)
                    .count(),
                2
            );
            let batch = game.query(&value_key).unwrap();
            let mut rows = batch.iter();
            assert!(rows.try_next().unwrap().is_some());
            assert!(rows.try_next().unwrap().is_none());
            game.remove_remote_dragon(420);
            // No prune pass between removal and network-ID reuse.
            assert!(!game.entity_attachments.contains_key(&421));
            assert!(!game.entity_initialization.initialized.contains(&holder));
            game.spawn_remote_dragon(420, [1.0, 80.0, 1.0], 0.0);
            assert!(!game.entity_attachments.contains_key(&421));
            assert!(
                game.query(&value_key)
                    .unwrap()
                    .iter()
                    .try_next()
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                value_key.get_existing(game, holder, |value| value.cloned()),
                Ok(Some(41))
            );
            assert_eq!(
                calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, h)| *h == holder)
                    .count(),
                3
            );
            let batch = game.query(&value_key).unwrap();
            let mut rows = batch.iter();
            assert!(rows.try_next().unwrap().is_some());
            assert!(rows.try_next().unwrap().is_none());
        });
    });
}
