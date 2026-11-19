//! A mod that links the game's wasmtime (`sunrise-wasmtime`) and whose `[[native.mixin]]` helper runs a module in it.
use sunrise_wasmtime::{Engine, Instance, Module, Store};

#[derive(Default)]
struct Mod;

#[mod_api::sunrise_mod]
impl mod_api::Hooks for Mod {}

/// `(module (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))`, as bytes: the
/// game's wasmtime has no `wat` feature.
const ADD: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
    0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, // type: (i32, i32) -> i32
    0x03, 0x02, 0x01, 0x00, // function 0 has that type
    0x07, 0x07, 0x01, 0x03, b'a', b'd', b'd', 0x00, 0x00, // export "add"
    // code: local.get 0, local.get 1, i32.add
    0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
];

/// Called by a helper: a mod's own engine compiles `ADD`, runs its `add` and prints the sum.
pub fn add(a: i32, b: i32) {
    let engine = Engine::default();
    let module = Module::new(&engine, ADD).expect("compile the module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiate the module");
    let add = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .expect("the module exports add");
    let sum = add.call(&mut store, (a, b)).expect("run add");
    eprintln!("fnr: wasmtime add {a}+{b}={sum}");
}
