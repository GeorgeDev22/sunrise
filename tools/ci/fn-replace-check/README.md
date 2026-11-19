Two native mods whose `[[native.mixin]]` rows the launcher builds against the prebuilt game by function
replacement (no game rebuild): `tools/ci/fn_replace_check.sh` builds them and runs a dedicated server.
Every helper prints a line starting `fnr:` so the run shows what ran, in what order.

`loop_generic_head` is built on its own, never beside `loop`: a HEAD mixin on the published loop's generic
function with no `[[native.loop]]` row, so the copy is the game's own loop with the empty slot.

`loop_second` is built beside `loop`, in the workflow's main set: a second mod's ticking block entities in the same world, and a slot of
its own that counts only those.

`wasmtime` depends on `sunrise-wasmtime`, the game's one wasmtime: its `[[native.mixin]]` helper calls a function of the mod crate that builds an `Engine`, compiles a module from bytes and runs its `add`, printing `fnr: wasmtime add 2+3=5`. It is in the main set, so the mod's link to the game's copy is checked from inside the game process.

`api_only` has hooks and an `[[inject]]` listener and no `[native]` table row. Alone (`FNR_ROUTE=prebuilt-api`) it
takes the API route: the package's prebuilt API executable with the launcher's `sunrise_native_mods` library. In the
main set, beside mods with `[[native.mixin]]` rows (`FNR_ROUTE=prebuilt`), the whole set takes the engine route.

`shared_left`, `shared_right` and `shared_tip` are their own set: the tip depends on the other two, and all three use
`itoa`. Each mod is a dylib, so the tip links only because the launcher builds the mods' registry crates once, into a
library they all depend on (sunrise-org/sunrise#1379). The check finds that library in the export, recorded as no
mod's, and with resources the server prints `fnr: shared_tip left 1 right 2 own 3`.
