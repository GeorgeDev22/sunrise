//! The `mob_spawned` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0048

pub trait Hooks {
    /// A new mob of `kind` joined `dimension` at `pos`: `natural` when the
    /// world's spawning made it, false when something else did (a breed, a
    /// raid, a spawner, a conversion, `/summon`, and the second mob of a
    /// jockey the world spawned). One call a mob: a `/summon` with passengers
    /// tells each. A natural spawn tells this notice for each body that remains
    /// resident in the source level after its join callback, before natural spawn
    /// accounting and the cluster-size callback. A join veto or transfer out of
    /// the source does not fire the notice for that body. Neither does loading a
    /// mob with its chunk, leaving a hive or changing level. It only hears;
    /// the mob is already among the game's mobs. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_mob_spawned",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        neoforge = "EntityJoinLevelEvent, FinalizeSpawnEvent",
        version = 2
    )]
    fn mob_spawned(dimension: &str, kind: &str, pos: [f64; 3], natural: bool);
}
