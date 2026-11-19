# Your first WebAssembly mod

A WebAssembly mod is the same Rust you write for a native mod, built to `mod.wasm` instead. The game runs it in a
sandbox, with limits on time and memory, so a player needs no compiler and no trust prompt for native code. In
this page you build the greeter from [Your first native mod](first-mod.md) as a WebAssembly mod.

You need `cargo sunrise-mod` ([Install the mod tool](install-tool.md))
and [rustup](https://rustup.rs): the first build installs the pinned Rust toolchain with `rust-src`.

## 1. Start from the native project

Make the project from the wasm template. It is the native project without the `[native]` table: a mod carries
native code or WebAssembly, never both, and the build writes its own `[wasm]` table. The SDK build the mod is
made for is pinned in `Cargo.toml` instead, under `[package.metadata.sunrise] sdk`
([the pin](../reference/sdk/03-in-a-project.md#the-pin)).

```sh
cargo sunrise-mod new greeter --template wasm
cd greeter
```

Keep the code in `src/lib.rs` as it is:

```rust,ignore
{{#include ../../examples/src/greeter.rs:mod}}
```

## 2. Build

```sh
cargo sunrise-mod build --wasm
```

The tool compiles the crate with the pinned toolchain, writes `mod.wasm` beside `sunrise-mod.toml`, records it in a
`[wasm]` table, and packs `target/sunrise-mod/greeter.smod`:

```toml
[wasm]
sha512 = "…"                    # mod.wasm's hash, checked when the game loads it
toolchain = "<version> <commit>" # every mod linked together is built by the same one
```

Never write `[wasm]` by hand; rebuild instead.

## 3. Play

Start the game with the package, from the `greeter` folder:

```sh
<path to sunrise-client> --dev-mod target/sunrise-mod/greeter.smod --resources <resources folder> \
  --game-dir target/sunrise-mod/game --home target/sunrise-mod/home
```

`sunrise-client` is the game, from the download for your computer in the
[Nightly release](https://github.com/sunrise-org/sunrise/releases/tag/nightly), and the resources folder is the one
[prepared from your own Minecraft](first-mod.md#prepared-resources), written as a full path. `--dev-mod` turns the
mod on for this start; `--game-dir` and `--home` keep the game's saves, settings and logs under the project's
`target/`. Join a world and the greeting appears.

To play it in a game folder you already use, copy `greeter.smod` into that folder's `sunrise-mods/` (the game folder
is the one that holds `options.txt`) and turn it on with a line `mod.greeter:true` in `sunrise-mods.txt` beside
`options.txt`. The game loads it at start-up.
If the game refuses the mod (a wrong layout, a missing dependency, a limit), the Mods screen shows why in red.

## What differs from native

Hooks, Parts, attachments, `[dependencies]` and the manifest mean the same on both targets. A WebAssembly mod
cannot use the [native route](../concepts/native-route.md) (`[native.*]` tables), and an API item marked
**Not on wasm** in the [API reference](../reference/api.md) is unavailable to it. Values cross between the game and
`mod.wasm` in one shared encoding; your own types cross when they derive [`Wire`](../concepts/wire-types.md).

[WebAssembly mods](../how-to/wasm-mods.md) covers dependencies, memory, limits and the module cache.
