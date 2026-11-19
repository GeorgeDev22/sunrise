//! A host that builds the Pulley interpreter enters a guest from an ordinary thread without any
//! caller having installed the stack policy first: the runtime settles it where it builds the
//! engine. A fresh process, since the policy is decided once per process.
use std::process::Command;

fn module() -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
        (@custom "dylink.0" "\01\04\00\00\00\00")
        (import "env" "memory" (memory 1))
        (import "env" "__indirect_function_table" (table 1 funcref))
        {log}
        (func (export "sunrise_in") (param i32) (result i32) i32.const 512)
        (func (export "sunrise_hook") (param i64) (result i64) i64.const 0))"#,
        log = sunrise_mod_wasm::fixture::log_import(),
    ))
    .unwrap()
}

fn child() {
    let bytes = module();
    // libtest-free and ordinary: far below the 136 MiB a native-code guest reserves.
    let entered = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || sunrise_mod_wasm::fixture::module(&bytes).map(|_| ()))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(entered, Ok(()));
    assert_eq!(
        sunrise_guest_stack::engine(),
        sunrise_guest_stack::GuestEngine::Interpreter
    );
    println!("pulley policy proof passed");
}

fn main() {
    if std::env::args().any(|arg| arg == "--child") {
        child();
        return;
    }
    if !(cfg!(target_pointer_width = "64") && cfg!(target_endian = "little")) {
        println!("pulley policy proof skipped: it names the pulley64 target");
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--child")
        .env("SUNRISE_FIXTURE_TARGET", "pulley64")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert!(text.contains("pulley policy proof passed"), "{text}");
}
