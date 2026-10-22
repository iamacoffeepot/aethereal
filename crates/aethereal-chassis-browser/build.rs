//! Links the compute example's dots shader, which imports a shared piece from the `aethereal_render` WESL package, into
//! the WGSL the example hands the renderer (ADR-0030).

use aethereal_render::shaders::PACKAGE;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut compiler = wesl::Compiler::default();
    compiler.options.dependencies.push(&PACKAGE);
    let dots = compiler.compile("examples/compute/dots.wesl")?;
    dots.emit_rerun_if_changed();
    dots.write_artifact("compute_dots");
    Ok(())
}
