//! The photons as the lens traces them: one quad over the whole target, drawn by a program whose fragment stage
//! follows each pixel's photon through the curved spacetime, in a pass of each of the two frames, both bound to the
//! orbit's view.

use crate::bytes::{floats, words};
use crate::passes::{Small, Trace};
use aethereal_actor::BlobRef;
use aethereal_data::Plain;
use aethereal_render::frame::{Frame, FrameId};
use aethereal_render::mesh::{Attribute, MeshId, VertexFormat, VertexLayout};
use aethereal_render::placement::{Bind, Drawn, Join, Place, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId,
};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::uniforms::UniformsId;
use aethereal_window::WindowId;

/// The program that traces the photons.
pub const TRACE_WGSL: &str = include_str!("../shaders/trace.wgsl");

/// The bytes of the view's uniforms.
const VIEW_BYTES: u32 = 32;

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// The quad's corners, from minus one to one, over the whole target, and the indices of its two triangles.
pub fn quad() -> (VertexLayout, Vec<u8>, Vec<u8>) {
    (corners(), floats([-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]), words([0, 1, 2, 0, 2, 3]))
}

/// The program of the WGSL the blob `wgsl` holds, which writes each pixel's traced light into a target of floats.
pub fn program(wgsl: BlobRef) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: VIEW_BYTES } }],
        state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None },
        shape: Shape::flat(ColourFormat::Rgba16Float),
    }
}

/// What traces the photons: the quad, the program, and the orbit's view.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Tracing {
    pub mesh: MeshId,
    pub program: ProgramId,
    pub view: UniformsId,
}

impl Tracing {
    /// The placement of the quad in the small pass of `glow` and in the trace pass of `window`'s frame, both traced
    /// from the one view.
    pub fn place(self, glow: FrameId, window: WindowId) -> Place {
        let bound = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(self.view) }];
        let joins = vec![
            Join::to::<Small>(Frame::Texture(glow), self.program, bound.clone()),
            Join::to::<Trace>(Frame::Main(window), self.program, bound),
        ];

        Place { mesh: self.mesh, instances: Drawn::Count(1), joins }
    }
}
