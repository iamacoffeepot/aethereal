//! The stars as the lens draws them: one quad, drawn once for each star by a program that reads the star its instance
//! names from storage, in two passes of two frames, each bound to uniforms of its own that size the sprites for its
//! target.

use crate::bytes::{floats, words};
use crate::camera::Camera;
use crate::disc::STARS;
use crate::passes::{Small, Stars};
use aethereal_actor::BlobRef;
use aethereal_data::Plain;
use aethereal_render::frame::{Frame, FrameId};
use aethereal_render::mesh::{Attribute, MeshId, VertexFormat, VertexLayout};
use aethereal_render::placement::{Bind, Drawn, Join, Place, Resource};
use aethereal_render::program::{
    Access, Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId,
};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::storage::StorageId;
use aethereal_render::uniforms::UniformsId;
use aethereal_window::WindowId;

/// The program that draws the stars.
pub const STARS_WGSL: &str = include_str!("../shaders/stars.wgsl");

/// The bytes of a star in storage, and of the view's uniforms.
const STAR_BYTES: u32 = 32;
const VIEW_BYTES: u32 = 48;

/// Half a star's sprite on the window, in its pixels, and the light each gives there.
const SHOWN_SPRITE: f32 = 1.7;
const SHOWN_GAIN: f32 = 0.16;

/// Half a star's sprite in the glow, in its pixels, and the light each gives there: larger for its smaller target, so
/// no star falls between its pixels.
const GLOW_SPRITE: f32 = 1.3;
const GLOW_GAIN: f32 = 0.2;

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// The quad's corners, from minus one to one, and the indices of its two triangles.
pub fn quad() -> (VertexLayout, Vec<u8>, Vec<u8>) {
    (corners(), floats([-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]), words([0, 1, 2, 0, 2, 3]))
}

/// The program of the WGSL the blob `wgsl` holds, which adds each star's light into a target of floats.
pub fn program(wgsl: BlobRef) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: vec![
            Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes: STAR_BYTES, access: Access::Read } },
            Binding { group: 0, binding: 1, kind: BindingKind::Uniform { bytes: VIEW_BYTES } },
        ],
        state: Fixed { blend: Blend::Additive, depth: Depth::Off, cull: Cull::None },
        shape: Shape::flat(ColourFormat::Rgba16Float),
    }
}

/// The bytes of the uniforms the stars are drawn into the glow with, through `camera` on its smaller target.
pub fn glow_view(camera: Camera) -> Vec<u8> {
    camera.view(GLOW_SPRITE, GLOW_GAIN)
}

/// The bytes of the uniforms the stars are drawn onto the window with, through `camera`.
pub fn shown_view(camera: Camera) -> Vec<u8> {
    camera.view(SHOWN_SPRITE, SHOWN_GAIN)
}

/// What draws the stars: the quad, the program, and each pass's uniforms.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Drawing {
    pub mesh: MeshId,
    pub program: ProgramId,
    pub glow: UniformsId,
    pub shown: UniformsId,
}

impl Drawing {
    /// The placement of every star of `stars`, in the glow's pass of `glow` and in the stars' pass of `window`'s frame.
    pub fn place(self, stars: StorageId, glow: FrameId, window: WindowId) -> Place {
        let bound = |view| {
            vec![
                Bind { group: 0, binding: 0, to: Resource::Storage(stars) },
                Bind { group: 0, binding: 1, to: Resource::Uniforms(view) },
            ]
        };
        let joins = vec![
            Join::to::<Small>(Frame::Texture(glow), self.program, bound(self.glow)),
            Join::to::<Stars>(Frame::Main(window), self.program, bound(self.shown)),
        ];

        Place { mesh: self.mesh, instances: Drawn::Count(STARS), joins }
    }
}
