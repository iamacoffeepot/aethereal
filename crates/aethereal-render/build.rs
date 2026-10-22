//! Packages the shared shader pieces in `shaders/` as the WESL package `aethereal_render`, for the build scripts of
//! the programs that import them (ADR-0030).

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=shaders");
    wesl::PackageBuilder::new("aethereal_render").scan_root("shaders")?.validate()?.build_artifact()?;
    Ok(())
}
