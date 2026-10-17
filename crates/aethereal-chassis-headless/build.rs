//! Builds the guests the tests run, for wasm32, into the build's own directory (ADR-0015).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        panic!("cargo sets OUT_DIR for a build script");
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests/echo/Cargo.toml");
    println!("cargo::rerun-if-changed=guests/echo");
    println!("cargo::rerun-if-changed=../aethereal-actor/src");
    println!("cargo::rerun-if-changed=../aethereal-data/src");
    let target = out.join("guests");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // The nested build gets an environment of its own: what the outer build set for this script is not for it.
    let status = Command::new(cargo)
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .status();
    let built = status.is_ok_and(|status| status.success());
    assert!(built, "the guests the tests run are built for wasm32");
    let wasm = target.join("wasm32-unknown-unknown/release/aethereal_guest_echo.wasm");
    println!("cargo::rustc-env=AETHEREAL_GUEST_ECHO={}", wasm.display());
}
