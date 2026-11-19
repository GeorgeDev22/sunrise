# The game's wasmtime: `sunrise-wasmtime`

A native mod that runs WebAssembly itself, such as a script host, takes the game's own `wasmtime` through the
SDK package `sunrise-wasmtime`. This is unstable native-route surface, like the tables on the other pages of
this section: it gives you real engine types, it is pinned to one game build, and it is not part of `mod_api`.

## The dependency

```toml
[dependencies]
sunrise-mod-api = { path = ".sunrise/sdk/api/crates/sunrise-mod-api" }
sunrise-wasmtime = { path = ".sunrise/sdk/api/crates/sunrise-wasmtime" }
```

It is named by the same path form as the other SDK packages ([In a mod project](../../reference/sdk/03-in-a-project.md));
every other form is refused under `E_SDK_SOURCE`. The crate re-exports everything `wasmtime` exports, so
`sunrise_wasmtime::Engine` is wasmtime's `Engine`. To keep writing `wasmtime::…` in your code, rename it:

```toml
wasmtime = { package = "sunrise-wasmtime", path = ".sunrise/sdk/api/crates/sunrise-wasmtime" }
```

```rust,ignore
use sunrise_wasmtime::{Engine, Instance, Module, Store};

let engine = Engine::default();
let module = Module::new(&engine, wasm_bytes)?;
let mut store = Store::new(&engine, ());
let instance = Instance::new(&mut store, &module, &[])?;
```

Your `Engine` and `Store` are your own and share nothing with the game's wasm mods.

## Features and version

You get the game's wasmtime exactly as the game builds it: version 50, default features off, and the features
`anyhow`, `cranelift`, `runtime`, `std`, `parallel-compilation`, `pulley` and `threads`. So there is no `wat`, `async`,
`component-model` or `cache`: compile modules from bytes. A dependency entry with its own `features` or
`default-features = false` is refused, since the features are not yours to choose.

The version follows the game build. When you move `[native] sunrise` to a new build, wasmtime may move with it:
rebuild, and the compiler names what changed.

## One copy, and why not to bring your own

The game process already links one wasmtime. Adding `wasmtime = "50"` from crates.io puts a second copy of the same
version in it: both export the same version-suffixed C symbols, and both install process-wide trap signal handlers,
so a trap in either depends on which loaded first. It also adds about 110 crates to what the player is asked to
allow, and compiles them all again. `sunrise-wasmtime` is the game's library, so the mod links that one copy, with
nothing extra to compile or approve.

## Who may use it

Only a native mod with a `[[native.*]]` row, because that mod is built for one exact game build, as is the wasmtime it
links. An API-only native mod (no `[[native.*]]` row) installs on any build where the surface items it uses are
unchanged, and a dependency on the game's wasmtime would break that, so the launcher refuses it under
`E_NATIVE_ONLY_PACKAGE`.

A wasm mod runs inside this wasmtime and cannot link it. `cargo sunrise-mod build` refuses a wasm mod that depends
on `sunrise-wasmtime`, under `E_WASM_NATIVE_PACKAGE`.

## Helpers and support modules

The dependency above is for your mod crate's own code. A `[[native.mixin]]` helper or a `[[native.module]]` is
compiled into the game's patch library, which already links every crate the game exposes, so it can name
`wasmtime` directly with no dependency line. Code that several helpers share belongs in your mod crate, where
`sunrise-wasmtime` is declared.

## Checking locally

`cargo check` in your project resolves `.sunrise/sdk/api/crates/sunrise-wasmtime` from the SDK's API source, which
compiles wasmtime from crates.io for the check only. The launcher's build binds the game's own copy.
