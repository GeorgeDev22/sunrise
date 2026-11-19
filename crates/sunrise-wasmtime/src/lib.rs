//! The game's own `wasmtime`, for native mods.
//!
//! The game links exactly one wasmtime, in the wasm mod runtime. A native mod that embeds a wasm engine
//! depends on this crate instead of `wasmtime` from crates.io, so the game process runs one copy: one set of trap
//! signal handlers, one code cache, no second compile of wasmtime and its dependencies.
//!
//! ```toml
//! sunrise-wasmtime = { path = ".sunrise/sdk/api/crates/sunrise-wasmtime" }
//! ```
//!
//! Everything wasmtime exports is re-exported here, so `sunrise_wasmtime::Engine` is wasmtime's `Engine`.
//! A mod that wants the crate's own name writes `wasmtime = { package = "sunrise-wasmtime", path = … }`.
//!
//! This is the native route's unstable surface:
//! - **Native mods with a `[[native.*]]` row only.** An API-only mod is not tied to one build, so the launcher refuses
//!   the dependency there; a wasm mod cannot link it and `cargo sunrise-mod build` refuses it too.
//! - **Pinned to the game build.** The version and the features below are the game's; a game update can change
//!   either, and the mod is rebuilt against it like any native mod.
//! - **Real engine types.** Nothing here is part of `mod_api`: a mod's `Engine` and `Store` are its own, and share
//!   nothing with the game's wasm mods.
//!
//! The wasmtime features enabled are `anyhow`, `cranelift`, `runtime`, `std`, `parallel-compilation`, `pulley`
//! and `threads`, with the defaults off. So there is no `wat`, `async`, `component-model` or `cache`: a mod
//! compiles modules from bytes.

pub use wasmtime::*;
