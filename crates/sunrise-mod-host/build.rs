use std::fmt::Write as _;
use std::process::Command;
use std::{env, fs, path::PathBuf};
fn main() {
    locales();
    println!("cargo:rerun-if-env-changed=SUNRISE_GAME_BUILD_ID");
    let commit = std::env::var("SUNRISE_GAME_BUILD_ID").unwrap_or_else(|_| {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git build identity");
        assert!(
            output.status.success(),
            "git rev-parse HEAD failed; set SUNRISE_GAME_BUILD_ID for staged builds"
        );
        String::from_utf8(output.stdout)
            .expect("git utf8")
            .trim()
            .to_owned()
    });
    assert!(
        !commit.trim().is_empty(),
        "game build identity must not be empty"
    );
    let rustc = Command::new(std::env::var_os("RUSTC").expect("RUSTC"))
        .arg("-vV")
        .output()
        .expect("rustc");
    let version = String::from_utf8(rustc.stdout).expect("rustc utf8");
    let release = version
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .expect("release");
    println!("cargo:rustc-env=SUNRISE_GAME_BUILD_ID={commit}");
    println!("cargo:rustc-env=SUNRISE_RUSTC_VERSION={release}");
}

/// This crate's lang files (`assets/lang/<code>.json`) as a static `Files` value, as
/// sunrise-reconnect's build does: the title screen's Mods button label travels with the crate
/// that supplies the row.
fn locales() {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/lang");
    println!("cargo:rerun-if-changed={}", source.display());
    let mut paths: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort_unstable();
    let mut list = String::from("&[\n");
    for path in paths {
        let locale = path.file_stem().unwrap().to_str().unwrap();
        writeln!(list, "    ({locale:?}, include_str!({:?})),", path).unwrap();
    }
    list.push_str("]\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("locales.rs"), list).unwrap();
}
