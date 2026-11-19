//! `entity_tick_pre` and `entity_tick_post` through the generated transport: a family's ids
//! and kinds cross to the guest in one call, and the ids its Pre cancels come back after those
//! the host already held.
use super::*;

pub(super) struct EntityTickPreHooks;
impl EntityTickPreHooks {
    pub(super) fn entity_tick_pre(
        &self,
        _: mod_api::HostSide<'_>,
        event: &mut mod_api::EntityTickPreEvent<'_>,
    ) {
        assert_eq!(event.dimension, "minecraft:the_nether");
        for (&entity, &kind) in event.entities.iter().zip(event.kinds) {
            if kind == "minecraft:item" {
                event.canceled.push(entity);
            }
        }
    }
    pub(super) fn entity_tick_post(
        &self,
        host: mod_api::HostSide<'_>,
        event: &mut mod_api::EntityTickPostEvent<'_>,
    ) {
        assert!(matches!(host, mod_api::HostSide::Client(_)));
        assert_eq!(event.dimension, "minecraft:the_end");
        assert_eq!(event.entities, [8, 9]);
        assert_eq!(event.kinds, ["minecraft:cow", "minecraft:zombie"]);
        POSTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// How many Posts the guest checked.
static POSTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct World;
impl mod_api::Host for World {}
mod_api::impl_host_areas!(World);
impl mod_api::ServerHost for World {}
mod_api::impl_server_host_areas!(World);
impl mod_api::ClientHost for World {}
mod_api::impl_client_host_areas!(World);

fn module() -> WasmMod {
    WasmMod {
        group: 0,
        subscribed: &[],
        listeners: &[],
        corrupt: false,
    }
}

#[test]
fn wire_loopback_entity_tick_pre_returns_the_familys_cancels() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let mut canceled = vec![1];
    module().entity_tick_pre(
        mod_api::HostSide::Server(&mut World),
        &mut mod_api::EntityTickPreEvent {
            dimension: "minecraft:the_nether",
            entities: &[5, 6, 7],
            kinds: &["minecraft:item", "minecraft:zombie", "minecraft:item"],
            canceled: &mut canceled,
        },
    );
    assert_eq!(canceled, [1, 5, 7]);
}

/// A client's Post crosses with the family's ids and kinds, in order.
#[test]
fn wire_loopback_entity_tick_post_lends_the_family_on_a_client() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let before = POSTS.load(std::sync::atomic::Ordering::Relaxed);
    module().entity_tick_post(
        mod_api::HostSide::Client(&mut World),
        &mut mod_api::EntityTickPostEvent {
            dimension: "minecraft:the_end",
            entities: &[8, 9],
            kinds: &["minecraft:cow", "minecraft:zombie"],
        },
    );
    assert_eq!(POSTS.load(std::sync::atomic::Ordering::Relaxed), before + 1);
}
