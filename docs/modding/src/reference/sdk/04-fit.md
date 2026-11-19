# Fitting another build

A mod is **made for** the build it pins. A native mod on the mod API alone also **fits** another build when nothing
it uses changed there, and the launcher then compiles it against that build on the player's computer. Only the built
binary is exact to the build; the loader's build-id check is unchanged.

## The rule

The launcher asks, for each enabled native mod of an instance, in this order:

1. **Made for this build?** Then it is built, as always.
2. **Exact by nature?** A mod with a `[native.*]` table (`mixin`, `module`, `bridge`, `state`, `loop`) or game
   internals (an `internals` list, or a dependency on `sunrise-internals`) changes or reads game code, so it works on
   the build it was made for only. No test runs.
3. **Does its package say what it uses?** Its `uses.json` must be one this launcher reads and be made for the mod's
   pin. A package without one (not packed by `cargo sunrise-mod build`, which writes it) works on the build it was made for only.
4. **Is every item it uses unchanged?** Each entry of the list must hold in this build's `surface.json`:
   - the record exists, with the same hash, under every public path the mod writes it by;
   - a type keeps every trait impl it had;
   - a struct the mod builds with a literal can still be built that way, and, built field by field (no `..`), still
     has the same fields;
   - a hook in the list still has the version it had;
   - a trait member the mod reaches through a trait it calls (a sub-host trait's method called on
     `&mut dyn ServerHost`) is still reached from that trait.
5. **Do its `[[inject]]` points hold?** Each row's point must be the same in this build's `points.json`.
6. **Do the mods it needs fit?** A mod whose library mod is left out is left out too: "it needs Corelib, which
   doesn't fit".
7. **Does it compile?** The compile against the build is the final check. A mod that fits and fails to compile is
   left out of that build, and the other mods build.

A mod that doesn't fit stays installed and is left out of the build, with the changed items named (at most five; the
build log has them all). The texts players see, the check before a game update and what happens to worlds saved with
such a mod are in [What the launcher does](../../how-to/native-mods/04-what-the-launcher-does.md#mods-made-for-another-build).

What fits does not promise: that the behaviour behind an unchanged declaration is the same. That is the API's
contract, as for any library.

## `uses.json`

`cargo sunrise-mod build` packs `uses.json` beside `sunrise-mod.toml` in every code mod's package, except a mod with
a `[native.*]` table or an `internals` list, or one whose source the scan cannot judge (the build says so and packs none). The list holds
every surface item whose name occurs as an identifier in the mod's source or that its manifest names, then every
item those items' signatures reference, transitively. A member counts only when its owner is reached. How the scan
reads source, and what it leaves to the compile, is in
[Checking a mod against a build](../../how-to/native-mods/14-checking-a-mod-against-a-build.md).

| Field | What it is |
|---|---|
| `schema` | The table's schema number (2). A reader also takes schema 1 ([Lists an older tool wrote](#lists-an-older-tool-wrote)). It refuses any other number, unknown fields, repeated ids and a table over 4 MiB. |
| `made_for` | The build id the hashes come from: the mod's pin. |
| `entries[].id`, `entries[].hash` | A record the mod uses, and its hash at `made_for`. |
| `entries[].impls` | A type's trait impls at `made_for`. |
| `entries[].built` | How the mod builds a struct it names: `full` (some literal or pattern lists its fields with no `..`), `base` (every one ends in `..`) or `none`. |
| `entries[].fields_hash`, `entries[].fields` | With `full`: the struct's field set at `made_for`, and its field names, to name what a build gained. |
| `entries[].built_at` | `file:line` of the first literal that builds it. |
| `entries[].paths` | The public paths the mod writes the item by, besides its id's. |
| `entries[].reached_from` | For a trait's member: the traits the mod calls that lead to its owner. |
| `entries[].version` | For a hook: its version at `made_for`. |

A struct built with `..` (for example `..Default::default()`) keeps fitting when the struct gains a field; one built
field by field does not. `cargo sunrise-mod check --uses` says which of the mod's literals are which.

A registry or a release page can tell which builds a published package fits from the package and each build's
`surface.json` alone: `cargo sunrise-mod fits <file.smod> --build <name>`. For a current list nothing from the build
the mod was made for is needed, so pruning that build's compiled files never strands the mod. `fits` first checks
that the list was made for the build the package pins (a native mod's `[native] sunrise`, a wasm mod's
`[package.metadata.sunrise] sdk` in its packed `Cargo.toml`). If the pin is missing, is not a full commit SHA,
or does not match `made_for`, the list cannot show which other builds the package fits.

The version promise covers the hooks in the list. A hook entry that holds no version cannot be shown to mean what
the mod was written for, whatever the target's version is: when nothing else changed, the answer is "not known",
naming the hooks, never "fits". `native-check` reports it as `no-list` with `why: "hook-versions"`.

### Lists an older tool wrote

A schema 1 list differs in two ways, and packing the mod again with `cargo sunrise-mod build` removes both:

- **It holds no hook versions.** `cargo sunrise-mod fits` fetches the `surface.json` of the build the list was
  made for only when a comparison with another build would otherwise be "not known" because a required hook
  version is missing. That file stays published when a build's compiled files are pruned. If it cannot be fetched,
  the answer is still "not known", with the reason. `native-check` and `sunrise-stage fits` do not fetch that file:
  both still report definite misfits and otherwise report "not known" when a required hook version is missing.
- **It recorded a struct as built wherever the scan was in doubt**, with nothing to tell a doubt from a literal the
  scan had placed. A reader does not read how such a list says a struct is built. This is a chosen trade-off: a
  struct the mod really builds field by field, which gained a field, is judged to fit and then fails the compile,
  which leaves the mod out by name. The other choice refuses mods that would have worked.

A list with a schema newer than the tool reads is `no-list` with `why: "newer-schema"`: a tool from a newer build
reads it.
