//! The shared shader pieces (ADR-0030): generic WGSL functions programs need, written in WESL and offered as the
//! package `aethereal_render`, which a program's build script links into its shader.
//!
//! | module | holds |
//! |---|---|
//! | `hash` | `hash(i, j, k, seed)`, the integer hash of a cell and a seed to a fraction in [0, 1), mixed as JavaScript's `Math.imul` mixes, the top 24 bits |
//! | `noise` | `noise(x, z, k, seed)`, value noise over the hash from -1 to 1, and `fbm`, its two-octave sum |
//! | `shape` | `smooth_step(a, b, x)` and `smooth_min(a, b, k)` |
//! | `colour` | `from_oklab`, Oklab to linear sRGB; `encode`, linear to sRGB encoding; and `tone`, the filmic tone curve |
//! | `grid` | `bilinear(uv, size)`, the four cells and two weights of a bilinear sample of a grid of floats in a storage buffer |
//!
//! A program imports what it uses by the package's name, and the linker brings what that uses in turn:
//!
//! ```wgsl
//! import aethereal_render::noise::fbm;
//!
//! @fragment
//! fn shade(@location(0) at: vec2<f32>) -> @location(0) vec4<f32> {
//!     return vec4(vec3(0.5 + 0.5 * fbm(at.x, at.y, 0, 7)), 1.0);
//! }
//! ```
//!
//! Its build script depends on `aethereal-render` and on `wesl`, links the shader with this package as a dependency,
//! and writes the WGSL into its `OUT_DIR`:
//!
//! ```ignore
//! fn main() {
//!     let mut compiler = wesl::Compiler::default();
//!     compiler.options.dependencies.push(&aethereal_render::shaders::PACKAGE);
//!     compiler.compile("src/shade.wesl").expect("the shader links").write_artifact("shade");
//! }
//! ```
//!
//! The program then hands the renderer `include_str!(concat!(env!("OUT_DIR"), "/shade.wgsl"))`. The linker renames
//! each imported declaration after its path (`fbm` above links as `_1aethereal_render_noise_fbm`), so a piece's names
//! never meet the program's own; the program's own declarations, its entry points among them, keep their names. Each
//! piece is declared `public`, as WESL requires of a declaration another package imports.
//!
//! `grid` does not read the buffer: a module of a package cannot name a storage buffer the program declares, and
//! passing one by pointer needs WGSL's `unrestricted_pointer_parameters`, which WebGPU does not enable by default. It
//! answers the four cells and the two weights, and the program reads and blends them:
//!
//! ```wgsl
//! import aethereal_render::grid::bilinear;
//!
//! @group(0) @binding(0) var<storage, read> heights: array<f32>;
//!
//! fn height_at(uv: vec2<f32>) -> f32 {
//!     let b = bilinear(uv, vec2(1000, 1000));
//!     let near = mix(heights[b.cells.x], heights[b.cells.y], b.weights.x);
//!     let far = mix(heights[b.cells.z], heights[b.cells.w], b.weights.x);
//!     return mix(near, far, b.weights.y);
//! }
//! ```

#[cfg(test)]
mod tests;

use wesl_core::{StaticPackage, StaticPackageModule};

include!(concat!(env!("OUT_DIR"), "/aethereal_render.rs"));
