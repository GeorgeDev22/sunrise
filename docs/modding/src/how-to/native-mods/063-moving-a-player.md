<!-- chapter: Moving a player -->
<!-- separator: 1 -->

### Moving a player

`player.entity().move_by(host, delta, cause)` (`EntityRef::move_by`, the call that moves any other entity, on a
`&mut dyn ServerHost`) moves a player by `delta` through the level as `Entity.move` would: blocks, other entities' colliders and the player's
collision exclusions stop it, and a step up is taken. It returns the displacement made. As for every entity, a
`delta` beyond 64 blocks on an axis is refused. The `entity_move` hook hears it once, for
the player and with `cause`, and a listener may replace or cancel it as for any other entity (a cancelled move is
`Some([0.0; 3])` and the client is told nothing); `cause` changes only what the hook hears, not the physics.

The server's copy moves at once, and the player's client is sent a relative teleport: it adds the displacement to where
it is when the packet arrives and keeps its motion and look, so what it walked in the meantime is kept. Until it
confirms, which a client does at the start of its next frame, the server drops the position of the player's own reports
(their rotation is kept, and nothing is counted against the player).

So `move_by` is for a one-off move, a launch or a pull, and not for every tick: a development build logs a warning when
one player is moved on 20 ticks in a row. For motion on every tick, use `player.entity().push(host, delta)` from the
server half of the mod, or move the player from the client half of the mod.

A player on a moving collider stays on it when the target is still over it, and lets go otherwise. The call is refused
(`None`, nothing moves) for a value that is not finite, a target outside the world, a player that is riding something,
a sleeper, a player without a connection, or a move the collision query cannot answer.

#### The client half

The client moves its own player. Write a prediction (a conveyor moving a player, boots that hover) from
`player_tick_pre` on `HostSide::Client`, when `host.local_player() == Some(event.player)`, before the player's own
movement:

- `host.move_local_player_by([x, y, z])` applies a displacement in blocks as `Entity.move(MoverType.SELF, delta)`.
  It steps up and collides as the player's own move does, including moving colliders and collision exclusions.
  It updates the ground flag and fall state, and has the usual effects on velocity. It returns `Ok(Some(delta))`
  with the displacement actually made. It returns `Ok(None)` without moving if there is no local player, the
  player is dead, sleeping or riding, or a component is non-finite or outside ±64 blocks. An unavailable world
  phase or a move the engine cannot finish returns `Err(LevelWriteError::Unavailable)`. Other hooks with a
  writable client world host, such as `client_tick_pre` and `input_key`, can also call it. The client does not
  fire the server's `entity_move` hook. A crossing notifies `entity_entering_section` at the position write,
  before the move updates the ground flag, even when another move returns the player to its starting section in
  the same tick.
- `host.set_local_player_velocity([x, y, z])` replaces the local player's velocity, in blocks per tick (`false`, and
  nothing changes, for a value that is not finite or with no player of its own).
- `host.set_local_player_on_ground(flag)` sets its on-ground flag, which this tick's jump decision reads and the move
  then sets again from the collision (`false` with no player of its own).

None of these calls sends a packet. The normal position report carries the result and the server checks it as it
checks any report, correcting a move its speed or collision check refuses. The server refuses a write of a player's
velocity or ground flag. A server's write of a player's motion reaches that player's client only through an explicit
motion packet, which replaces the client's velocity outright.

At `player_tick_pre` the player's input fields hold the previous tick's applied values, so to scale this tick's forward
input (Create's boots multiply a slow one by 1.3) use the `movement_input` hook, which runs after the keys are read
and before they are applied.
