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
//! | `scan` | `scan(value, local)`, the exclusive and inclusive prefix sums of `u32` across a workgroup and its total; `scan_from`, the same counted from a carry; and `WORKGROUP_SIZE` |
//! | `compact` | `place(keep, local, first)`, where an ordered compaction writes each element a predicate keeps, and where the workgroup's kept elements end |
//! | `segment` | `Segment`, two `vec2<f32>`, and `along`, `closest_point`, `distance_to`, `contains_point`, `side`, and `crosses`, with the meanings of `aethereal_math::Segment`; its sides are the `i32` constants `SIDE_POSITIVE`, `SIDE_NEGATIVE`, and `SIDE_ON` |
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
//!
//! `scan` and `compact` read and write no buffer either, for the same reason. Each invocation hands its value, or
//! whether its element is kept, and its `@builtin(local_invocation_index)`, and the sums come back through workgroup
//! memory the module declares. Their workgroup is a fixed [`WORKGROUP_SIZE`], 256 invocations: the renderer refuses a
//! constant a pipeline overrides (ADR-0026), and 256 is the most WebGPU lets every device run. A program declares its
//! entry point with the shader's constant, `@workgroup_size(WORKGROUP_SIZE)`, and counts its workgroups with this one.
//! Every invocation calls them alike, as they wait at the workgroup's barriers: one past the end of the elements hands
//! 0, or keeps nothing, and does not return early.
//!
//! Compacting more than one workgroup's elements is three dispatches, each joining a compute pass of its own, as the
//! dispatches of one pass run in no order (ADR-0026). The first counts what each workgroup keeps into `totals`, one a
//! workgroup; the second, one workgroup, scans `totals` in place, a width at a time with a carry, into each
//! workgroup's offset, and writes the count where an indirect draw reads it; the third writes each kept element at its
//! workgroup's offset and its place within it:
//!
//! ```wgsl
//! import aethereal_render::scan::{WORKGROUP_SIZE, scan_from};
//! import aethereal_render::compact::place;
//!
//! @group(0) @binding(0) var<storage, read> items: array<vec2<f32>>;
//! @group(0) @binding(1) var<storage, read_write> totals: array<u32>;
//! @group(0) @binding(2) var<storage, read_write> kept: array<u32>;
//! @group(0) @binding(3) var<storage, read_write> count: u32;
//!
//! fn keeps(at: u32) -> bool {
//!     return at < arrayLength(&items) && length(items[at]) < 0.75;
//! }
//!
//! @compute @workgroup_size(WORKGROUP_SIZE)
//! fn tally(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
//!          @builtin(workgroup_id) group: vec3<u32>) {
//!     let placed = place(keeps(id.x), local, 0u);
//!     if local == 0u {
//!         totals[group.x] = placed.end;
//!     }
//! }
//!
//! @compute @workgroup_size(WORKGROUP_SIZE)
//! fn offsets(@builtin(local_invocation_index) local: u32) {
//!     let groups = arrayLength(&totals);
//!     var carry = 0u;
//!     for (var first = 0u; first < groups; first += WORKGROUP_SIZE) {
//!         let at = first + local;
//!         var total = 0u;
//!         if at < groups {
//!             total = totals[at];
//!         }
//!         let summed = scan_from(total, local, carry);
//!         if at < groups {
//!             totals[at] = summed.exclusive;
//!         }
//!         carry = summed.total;
//!     }
//!     if local == 0u {
//!         count = carry;
//!     }
//! }
//!
//! @compute @workgroup_size(WORKGROUP_SIZE)
//! fn scatter(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
//!            @builtin(workgroup_id) group: vec3<u32>) {
//!     let keep = keeps(id.x);
//!     let placed = place(keep, local, totals[group.x]);
//!     if keep {
//!         kept[placed.at] = id.x;
//!     }
//! }
//! ```
//!
//! The program makes `totals` one word for each workgroup, `items.div_ceil(WORKGROUP_SIZE)`, so `offsets` reads their
//! number from its length, and builds a frame with three compute passes before the one that draws what was kept:
//!
//! ```ignore
//! let mut frame = FrameBuilder::on(Ends::Window(window));
//! frame.compute(Tally);
//! frame.compute(Offsets);
//! frame.compute(Scatter);
//! let create = frame.finish(Main, Onto(depth::Off, ()))?;
//!
//! Dispatch::to::<Tally>(frame, tally, bindings)      // Invocations::PerElement(items)
//! Dispatch::to::<Offsets>(frame, offsets, bindings)  // Invocations::Workgroups([1, 1, 1])
//! Dispatch::to::<Scatter>(frame, scatter, bindings)  // Invocations::PerElement(items)
//! ```
//!
//! Up to [`WORKGROUP_SIZE`] elements need only the one dispatch: `place(keep, local, 0u)` answers each kept element's
//! place and, as `end`, the count. The browser example `indirect` compacts its items in the three passes above, and
//! draws them by the count.

#[cfg(test)]
mod tests;

/// The workgroup size of `scan` and `compact`, `WORKGROUP_SIZE` in the shader, for the program's side of a dispatch:
/// the workgroups that cover its elements, `elements.div_ceil(WORKGROUP_SIZE)`, and the totals they write, one each.
pub const WORKGROUP_SIZE: u32 = 256;

use wesl_core::{StaticPackage, StaticPackageModule};

include!(concat!(env!("OUT_DIR"), "/aethereal_render.rs"));
