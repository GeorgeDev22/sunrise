# Runtime levels

A mod opens a level of its own while the server runs and closes it again: an instanced dungeon, a
minigame arena, a pocket dimension per player. `host.runtime_levels()` has `open`, `unload`, `delete`
and `is_open`.

In `Hooks::register_commands`, bind an operator-only [server command](../entities/server-commands.md).
Return `Ok(1)` after `open` accepts the request, or a failure message if it refuses:

```rust,ignore
event.commands.push(Command::op("openarena", 2).executes(|host, _, _| {
    let level = RuntimeLevel::new("mymod:pocket", RuntimeGenerator::Mod("mymod:arena".into()), 4242);
    // On success the level opens after this tick's levels have ticked.
    host.runtime_levels().open("mymod:instance/ab12", level)
        .map_err(|_| Text::translate("mymod.commands.arena_open_failed", vec![]))?;
    Ok(1)
}));
```

## What a level is made of

- **The id** is `namespace:path` in a namespace the mod owns (its own id, or one its manifest
  lists). The folder is `dimensions/<namespace>/<path>/` of the world, derived from the id and
  nothing else, so an id that is not a folder name (`mymod:../x`, an empty segment) is refused
  with `InvalidId`, and one in another mod's namespace with `OutsideNamespace`. A vanilla level, a
  level a mod registered at startup and a level a data pack gave are `ReservedId`.
- **`dimension_type`** is a type known since startup: a vanilla type (`minecraft:overworld`) or the
  type of a level a mod registered with [`register_dimension`](dimension-shape.md#registering-a-dimension)
  (`mymod:pocket`). Nothing new crosses the network, so every client already has it. Any other
  name is `UnknownDimensionType`; a data pack's own dimension types are not offered yet.
- **`generator`** is `RuntimeGenerator::Flat` (bedrock, two dirt, a grass block, over plains),
  `Void` (air), or `Mod(name)`, a chunk generator a native mod registered
  ([`register_chunk_generator`](dimension-shape.md#chunk-generators)). An unregistered generator returns `UnknownGenerator`. A noise
  generator is `NoiseUnsupported` for now.
- **`seed`** is the level's own. It reaches the mod generator (`ChunkAccess::seed`), the level's block-tick
  and level randoms and the hashed seed clients are sent. It does not change structures or
  biomes. `ChunkAccess::level` tells a generator which level it fills, so one generator serves
  every instance and looks its plan up by id.

## When it happens

`open`, `unload` and `delete` check the request when they are called and return the refusal at once. The
change itself waits: the server applies the queue after its levels have ticked in the server
tick, never inside a level's tick, so no hook sees a level appear or vanish under it. Until then
`is_open` is false after an `open` and true after an `unload` or a `delete`. A request made while the queue is
applied waits for the next tick. `level_load` and `level_unload` fire as for any level the game
opens or closes, once each.

`open` returns a `LevelRef` that names this open of the level. A handle from before an `unload`
never names a later open of the same id: it reads as stale, like a handle from an earlier world.
A folder that exists under the id is reopened as it was saved, a persistent level after an
unload or a restart. Nothing reopens a runtime level by itself at the next start: the mod
reopens what it keeps, and a player saved in a level that was not reopened keeps the usual
fallback. At server startup every configured level (including datapack dimensions) fires
`level_load` (v4) before the next is available. The Overworld opens first and, if needed, selects the
initial world spawn before the other levels open, waiting for the spawn search's chunks to reach
FULL. The owner's player spawn is prepared separately. `create_spawn_position` runs during the
world spawn search for normal fresh worlds; the Debug preset uses its fixed spawn without that hook. The remaining levels follow
the `LEVEL_STEM` registry traversal; mods must not assume a particular order among them.
`host.levels().list_levels()`, ticks of eligible levels, saves and shutdown use that creation order.
A player's saved dimension does not add to the registry. The server then validates and activates
saved tickets and waits for the ticketed chunks before `server_started` (v2), even if no player visits
those levels. In `Hooks::validate_tickets`, a controller's owning mod can remove saved tickets
through `event.validation`. The callback sees its removals immediately. As with
[staged registration](../../concepts/load-phases.md#registration-and-setup), a panic or phase
refusal discards those edits before the saved tickets take hold. Other controllers still validate.

On a full server save, `player_save_to_file` notifications for successfully saved placed players
come before `level_save`, then `world_saved` follows the world metadata write. During shutdown,
a failed player file write emits no `player_save_to_file` for that player; it does not prevent the
world metadata save. Startup's integrated
save completes before `server_starting` and `server_started`. It writes levels and world metadata,
naming a known profile for a new world without creating or overwriting the owner's player file.
The initial save is outside the `server_started` listener's transaction journal.

Lifecycle hooks follow the [shared failure rules](../../concepts/hooks-and-the-host.md#failures-and-panics).
A panicking listener does not fail a successful save or skip another listener's level close
notice. On the server, the existing transaction journal discards that callback's journaled
writes, including edits committed by a nested `host.transaction`. Effects outside the journal,
such as a mod writing its own file or editing client state, are not undone.

On the client, resolve the level in `client_level_load` with `host.level(event.dimension)` before it
is installed. Its chunks arrive later, but it can already read the datapack registries synchronized
by its server for that configuration. On dimension replacement, `client_level_unload` follows
for the retiring level: its handle, loaded chunks and available synchronized registries still
resolve until that callback finishes. On reconfiguration, Unload reads the retiring configuration
and the next Load reads the new one, even if the new registry set arrived before Unload ran.
The new level's handle remains valid when that level is installed.

Success means queue admission, not apply completion. Apply failures are logged with the
requesting mod and level id. A failed open rolls back publication and frees the id for retry;
a failed unload leaves the level open for another unload request. Observe `is_open` after
the queue runs rather than treating the returned result as completion.

An explicitly opened runtime level fires `level_load` while parked: `event.dimension` names
the opened level, whereas the Host still names the current level.

## Closing

`unload(id)` saves the level and drops it, keeping the folder. It refuses with `PlayersInside`
while a player, or an arrival that has not landed, is in the level, both when it is called and
when it is applied: move the players out first with `entity.teleport_to_level`. `NotOpen` is the
answer for a level that is not open (or still waiting to open). A parked non-Overworld level that
has no players, views, tickets, or other work keeping it active can evict its chunks and entities
without closing: `is_open` stays true, neither `level_unload` nor `level_save` fires, and revisiting
it does not fire `level_load` again. An unload listener that activates another level keeps it
resident before its turn. The sweep leaves a column and its live entities resident if a required
record cannot be saved; other eligible columns can still evict. Only `unload`, `delete`, or server
shutdown closes the level and fires `level_unload`.

## Deleting

`delete(id)` removes a level for good: its folder `dimensions/<namespace>/<path>/` and the
structure references the game keeps beside the world for it
(`data/sunrise_structure_references_<namespace>_<path>.json`, kept if another id would share that
file and its level exists: `a_b:c` and `a:b_c` do), then the folders those leave empty. It works for the levels of the calling mod's namespace only, with the same refusals as
`unload` (`InvalidId`, `OutsideNamespace`, `ReservedId`), and the folder it removes comes from
the validated id and nothing else.

- A level that is **open** is unloaded first, as by `unload` (saved, `level_unload` fires), and
  its files are removed in the same pass. It refuses with `PlayersInside` while a player or an
  arrival that has not landed is in the level, when called and again when applied: move the
  players out first. `is_open` stays true until the delete is applied, which closes it.
- A level that is **not open** is only files, the leftovers of a crash or of an earlier
  `unload`. They are removed, and a level with no files is not an error. Nothing fires, since
  nothing was open.
- A level still **waiting to open** refuses with `AlreadyOpen`: its `open` comes first.
- A level with a **level below it** refuses with `LevelsBelow`, when called and again when
  applied. Folders nest: `mymod:instance/ab` lives inside the folder of `mymod:instance`, so
  removing the parent would remove the child, which may be open, with players or a save waiting.
  A level is below when its id continues the id with `/` and it is open, waiting, a level of the
  world, or has a folder under the level's own. A folder under it is any folder other than
  `region`, `entities`, `poi` and `data`, one of the first three holding a folder, or a folder in
  `data` with a level's shape somewhere below it (a folder holding `region`, `entities` or `poi`,
  or a link); the other files a mod keeps in `data` are not in the way. A folder that cannot be
  read counts as a level below. Delete the level below first.

The removal happens on the server thread, in the pass that applies the queue, after the level
has been saved and dropped, so no save is running against the files. A request in the queue is
applied in the order it was made. A reopen therefore waits for the removal: an `open` of a level
that is open and being deleted is `AlreadyOpen` until the delete is applied; an `open` of a level
that was not open, made after the `delete`, is applied after it and starts from nothing. A save a
mod asked for with `level_request_save` is dropped with the level.

A link (symbolic link, junction) on the way to the folder under `dimensions`, or to the
references file under `data`, is never followed: the removal fails before anything is removed.

If the removal fails (a link, a file another program holds), the failure is logged with the mod
and the id, and what happened to the level depends on how far the delete got. A level that could
not be closed, or has a level below it, stays open and `is_open` stays true; delete it again.
A level that closed but whose files could not all be removed stays **closed**: `is_open` is
false, its handle is stale, `open` of the id answers `Removing`, and an `open` already waiting
behind the delete is dropped with a log line instead of starting from the files that are left.
A second `delete` of the id finishes the removal, and only then can it be opened.

A handle from before the delete reads as stale, as after an `unload`.

## Players and clients

A player goes into a runtime level and out of it as into any level, with
`entity.teleport_to_level`. Its client, vanilla or Sunrise, learns the level on the way in: the
Respawn names it and its dimension type, which the client has had since it joined. On the way out
a Sunrise client forgets a level the server opened after it logged in, so a long session through
many instances keeps none of them. A level opened again under the same id after a `delete`
reaches the client afresh, with nothing of the old one.

A runtime level opened after a player logged in is viewed through [chunk areas](../entities/chunk-areas.md)
like any other level, by a Sunrise client of this version. A client of an older version names the
levels it views by their place in the list it got at login, where such a level is not: the areas
there are kept, but nothing of the level is sent to it, and it sees the level only by going in.
When a runtime level closes, by `unload` or `delete`, every client that viewed it drops its world
of it; the areas stay, and show the level again once it is open again.

## The cap

A server keeps at most 64 runtime levels open or waiting, whichever mods opened them: the
65th `open` is `CapReached`. A server sets another number with `sunrise-max-runtime-levels` in
`server.properties`; a server run by `sunrise-server` sets it under `[properties]` in `server.toml`
([hosting](https://github.com/sunrise-org/sunrise/blob/main/docs/hosting.md)). An unloaded or deleted level frees its place when the request is applied; idle chunk eviction does not close the level or free its place.

Every refusal is also logged with the name of the mod that asked.
