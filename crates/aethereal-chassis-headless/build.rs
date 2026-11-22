//! Builds the guests the tests run, for wasm32, into the build's own directory (ADR-0015).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        panic!("cargo sets OUT_DIR for a build script");
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests/echo/Cargo.toml");
    println!("cargo::rerun-if-changed=guests/echo");
    println!("cargo::rerun-if-changed=../aethereal-actor/src");
    println!("cargo::rerun-if-changed=../aethereal-data/src");
    let first = build(&manifest, &out.join("guests"), "");
    println!("cargo::rustc-env=AETHEREAL_GUEST_ECHO={}", first.display());
    // A second build of the same guests, with their second version, which a test moves them onto (ADR-0054). A cfg
    // selects it, as a feature would be turned on by a build of every feature, the first's types among them.
    let second = build(&manifest, &out.join("guests-v2"), "--cfg echo_v2");
    println!("cargo::rustc-env=AETHEREAL_GUEST_ECHO_V2={}", second.display());

    // Spike: the screen guest, written with `view!` (ADR-0056). `aethereal_guest` keeps the renderer's browser backend,
    // and with it wasm-bindgen's imports, out of a guest, which is wasm32 as a browser host is.
    let screen = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests/screen/Cargo.toml");
    println!("cargo::rerun-if-changed=guests/screen");
    println!("cargo::rerun-if-changed=../aethereal-ui/src");
    println!("cargo::rerun-if-changed=../aethereal-render/src");
    println!("cargo::rerun-if-changed=../aethereal-window/src");
    let built = build_named(&screen, &out.join("guest-screen"), "--cfg aethereal_guest", "aethereal_guest_screen");
    println!("cargo::rustc-env=AETHEREAL_GUEST_SCREEN={}", built.display());
}

/// Builds the guest whose manifest is `manifest` for wasm32 into `target`, with `flags` for the compiler, and answers
/// with where its module is.
fn build(manifest: &Path, target: &Path, flags: &str) -> PathBuf {
    build_named(manifest, target, flags, "aethereal_guest_echo")
}

/// Builds the guest whose manifest is `manifest`, whose module is `name`, as [`build`] does.
fn build_named(manifest: &Path, target: &Path, flags: &str, name: &str) -> PathBuf {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // The nested build gets an environment of its own: what the outer build set for this script is not for it.
    let status = Command::new(cargo)
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(manifest)
        .arg("--target-dir")
        .arg(target)
        .env("RUSTFLAGS", flags)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .status();
    let built = status.is_ok_and(|status| status.success());
    assert!(built, "the guests the tests run are built for wasm32");

    target.join(format!("wasm32-unknown-unknown/release/{name}.wasm"))
}
