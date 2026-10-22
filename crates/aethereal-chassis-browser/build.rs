//! Links the examples' shaders that import shared pieces from the `aethereal_render` WESL package into the WGSL each
//! example hands the renderer (ADR-0030): the compute example's dots, and the indirect example's cull.

use aethereal_render::shaders::PACKAGE;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut compiler = wesl::Compiler::default();
    compiler.options.dependencies.push(&PACKAGE);
    let dots = compiler.compile("examples/compute/dots.wesl")?;
    dots.emit_rerun_if_changed();
    dots.write_artifact("compute_dots");
    let cull = compiler.compile("examples/indirect/cull.wesl")?;
    cull.emit_rerun_if_changed();
    cull.write_artifact("indirect_cull");
    Ok(())
}
