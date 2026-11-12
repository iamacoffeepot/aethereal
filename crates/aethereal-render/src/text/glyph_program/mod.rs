//! The glyph program (ADR-0052): the renderer's own WGSL, drawing a quad for each glyph of a text, an instance each,
//! sampled from the atlas. The fragment stage takes the median of the field's three channels, the distance to the
//! glyph's edge, and turns it to coverage by how many of the frame's pixels a texel covers there, read off the
//! screen-space derivative, so the edge is one pixel wide at every size; an outline is drawn at a second distance.
//! A pipeline is made for each shape of pass a text joins.

#[cfg(test)]
mod tests;

use super::atlas::Slot;
use super::layout::LaidGlyph;
use super::{Clip, Colour, Layer, Outline};
use crate::font::msdf::{BASE_PIXELS_PER_EM, DEFAULT_PADDING};
use crate::frame::{DepthUse, Size};
use crate::picking;
use crate::shape::{DepthFormat, Shape};
use aethereal_math::Vec2;

/// The glyph program. Each instance is a glyph: where its quad's top left is from the text's, and its size, in the
/// frame's pixels; where its field lies in the atlas, in texels; and its run's colour. The uniforms are the
/// placement's in the pass: the frame's size, where the text is, its clip, its outline's colour, its opacity, its
/// depth, its outline's width in pixels, and how many texels the field reaches from 0 to 1.
pub const GLYPH: &str = "
struct Placed {
    frame: vec2<f32>,
    at: vec2<f32>,
    clip: vec4<f32>,
    rim: vec4<f32>,
    opacity: f32,
    depth: f32,
    rim_width: f32,
    span: f32,
}

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var smoothly: sampler;
@group(1) @binding(0) var<uniform> placed: Placed;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) pixel: vec2<f32>,
    @location(2) colour: vec4<f32>,
}

@vertex
fn corner(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) field: vec4<f32>,
    @location(3) colour: vec4<f32>,
) -> Corner {
    let right = index == 1u || index == 2u || index == 4u;
    let down = index == 2u || index == 4u || index == 5u;
    let unit = vec2(select(0.0, 1.0, right), select(0.0, 1.0, down));
    let pixel = placed.at + at + unit * size;
    let clip = vec2(pixel.x / placed.frame.x * 2.0 - 1.0, 1.0 - pixel.y / placed.frame.y * 2.0);
    return Corner(vec4(clip, placed.depth, 1.0), field.xy + unit * field.zw, pixel, colour);
}

fn median(channels: vec3<f32>) -> f32 {
    return max(min(channels.r, channels.g), min(max(channels.r, channels.g), channels.b));
}

@fragment
fn coverage(corner: Corner) -> @location(0) vec4<f32> {
    let texels = vec2<f32>(textureDimensions(atlas));
    let distance = median(textureSample(atlas, smoothly, corner.texel / texels).rgb);
    let per_texel = dot(vec2(0.5), 1.0 / max(fwidth(corner.texel), vec2(1e-6)));
    let pixels = (distance - 0.5) * placed.span * per_texel;
    let fill = clamp(pixels + 0.5, 0.0, 1.0) * corner.colour.a;
    let rim = clamp(pixels + placed.rim_width + 0.5, 0.0, 1.0) * placed.rim.a;
    let alpha = fill + rim * (1.0 - fill);
    let colour = (corner.colour.rgb * fill + placed.rim.rgb * rim * (1.0 - fill)) / max(alpha, 1e-6);
    let inside = all(corner.pixel >= placed.clip.xy) && all(corner.pixel < placed.clip.zw);
    if !inside {
        discard;
    }
    return vec4(colour, alpha * placed.opacity);
}
";

/// The entry point of [`GLYPH`]'s vertex stage.
pub const VERTEX: &str = "corner";

/// The entry point of [`GLYPH`]'s fragment stage.
pub const FRAGMENT: &str = "coverage";

/// The group [`GLYPH`] binds the picking step in, after the atlas's and a placement's (ADR-0055).
pub const PICKING: u32 = 2;

/// The bytes of one glyph's instance: two floats of where, two of its size, four of its field, four of its colour.
pub const INSTANCE_BYTES: usize = 48;

/// The bytes of a placement's uniforms in one pass.
pub const PLACED_BYTES: usize = 64;

/// How many texels a field reaches across its edge, from 0 to 1.
// The padding is a few texels, which an f32 holds exactly.
#[allow(clippy::cast_precision_loss)]
const SPAN: f32 = (2 * DEFAULT_PADDING) as f32;

/// A clip that cuts nothing: a box larger than any frame.
const UNCLIPPED: [f32; 4] = [-1.0e30, -1.0e30, 1.0e30, 1.0e30];

/// What every pipeline of the glyph program is made from: its module, with the picking step added, the layouts of its
/// two groups, the atlas's and a placement's, the layout of its pipelines, which binds the step's group after them,
/// and the sampler it reads the atlas through.
pub struct Layouts {
    module: wgpu::ShaderModule,
    pub atlas: wgpu::BindGroupLayout,
    pub placed: wgpu::BindGroupLayout,
    pipeline: wgpu::PipelineLayout,
    pub sampler: wgpu::Sampler,
}

impl Layouts {
    /// The glyph program's module and layouts, on `device`, binding the picking step through `picking`.
    #[must_use]
    pub fn new(device: &wgpu::Device, picking: &wgpu::BindGroupLayout) -> Self {
        // The step is added to the renderer's own program as to any, which its test proves; were it not, text would
        // draw, and only go unpicked.
        let wgsl = picking::picked(GLYPH, (VERTEX, FRAGMENT), PICKING).unwrap_or_else(|_| String::from(GLYPH));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let atlas = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let placed = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(PLACED_BYTES as u64),
                },
                count: None,
            }],
        });
        let pipeline = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&atlas), Some(&placed), Some(picking)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self { module, atlas, placed, pipeline, sampler }
    }
}

/// The glyph program's pipelines for one shape of pass: over what is drawn, for a shape that holds no depth; and for
/// one that holds depth, over it, and at a depth, tested, or tested and written.
#[derive(Clone)]
pub enum Pipelines {
    Flat { over: wgpu::RenderPipeline },
    Deep { over: wgpu::RenderPipeline, test: wgpu::RenderPipeline, write: wgpu::RenderPipeline },
}

/// How a pipeline of the glyph program uses depth.
#[derive(Clone, Copy)]
enum Depthed {
    /// The shape holds none.
    Flat,
    /// The shape holds depth: it is compared as `compare` says, and written when `writes`.
    Deep { compare: wgpu::CompareFunction, writes: bool },
}

impl Pipelines {
    /// The glyph program's pipelines for passes of `shape`, on `device`.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &Layouts, shape: Shape) -> Self {
        let made = |depthed| pipeline(device, layouts, shape, depthed);
        match shape.depth {
            DepthFormat::None => Self::Flat { over: made(Depthed::Flat) },
            DepthFormat::Depth32Float => Self::Deep {
                over: made(Depthed::Deep { compare: wgpu::CompareFunction::Always, writes: false }),
                test: made(Depthed::Deep { compare: wgpu::CompareFunction::Less, writes: false }),
                write: made(Depthed::Deep { compare: wgpu::CompareFunction::Less, writes: true }),
            },
        }
    }

    /// The pipeline a text placed as `layer` draws with in a pass that uses depth as `depth` says.
    #[must_use]
    pub const fn drawing(&self, layer: Layer, depth: DepthUse) -> &wgpu::RenderPipeline {
        match (self, layer, depth) {
            (Self::Flat { over }, ..)
            | (Self::Deep { over, .. }, Layer::Over, _)
            | (Self::Deep { over, .. }, Layer::At { .. }, DepthUse::Off) => over,
            (Self::Deep { test, .. }, Layer::At { .. }, DepthUse::Test(_)) => test,
            (Self::Deep { write, .. }, Layer::At { .. }, DepthUse::Write(_)) => write,
        }
    }
}

/// The glyph program's pipeline for passes of `shape`, using depth as `depthed` says, blending by alpha.
fn pipeline(device: &wgpu::Device, layouts: &Layouts, shape: Shape, depthed: Depthed) -> wgpu::RenderPipeline {
    let attributes = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 2 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 3 },
    ];
    let instances = wgpu::VertexBufferLayout {
        array_stride: INSTANCE_BYTES as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attributes,
    };
    let depth_stencil = match depthed {
        Depthed::Flat => None,
        Depthed::Deep { compare, writes } => Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(writes),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
    };
    let target = wgpu::ColorTargetState {
        format: shape.colour.wgpu(),
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layouts.pipeline),
        vertex: wgpu::VertexState {
            module: &layouts.module,
            entry_point: Some(VERTEX),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(instances)],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil,
        multisample: wgpu::MultisampleState { count: shape.samples.count(), ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: &layouts.module,
            entry_point: Some(FRAGMENT),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The instance of `glyph`, a glyph of a text of `size` pixels to the em, whose field lies at `slot`, in the bytes the
/// glyph program reads: none for a glyph that draws nothing, as a space.
#[must_use]
pub fn instance(glyph: &LaidGlyph, slot: Slot, size: f32) -> Option<[u8; INSTANCE_BYTES]> {
    let field = slot.field;
    if field.is_empty() {
        return None;
    }
    let factor = size / BASE_PIXELS_PER_EM;
    // A field's place and size are some hundreds of texels, and a slot's some thousands, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let values = [
        (field.left as f32).mul_add(factor, glyph.origin.x),
        (field.top as f32).mul_add(-factor, glyph.origin.y),
        field.width as f32 * factor,
        field.height as f32 * factor,
        slot.x as f32,
        slot.y as f32,
        field.width as f32,
        field.height as f32,
    ];

    Some(bytes(values.into_iter().chain(glyph.colour.channels())))
}

/// Where and how a placed text is drawn, as its uniforms say beside the frame's size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placing {
    pub at: Vec2,
    pub layer: Layer,
    pub opacity: f32,
    pub clip: Clip,
}

/// A placement's uniforms in a pass of a frame `frame` pixels in size, drawn as `placing` says, of a text whose
/// outline is `outline`, in the bytes the glyph program reads.
#[must_use]
pub fn placed(frame: Size, placing: Placing, outline: Outline) -> [u8; PLACED_BYTES] {
    let clip = match placing.clip {
        Clip::None => UNCLIPPED,
        Clip::To(rect) => [rect.min.x, rect.min.y, rect.max.x, rect.max.y],
    };
    let (rim, rim_width) = match outline {
        Outline::None => (Colour::CLEAR, 0.0),
        Outline::Of { width, colour } => (colour, width),
    };
    let depth = match placing.layer {
        Layer::Over => 0.0,
        Layer::At { depth } => depth,
    };
    // A frame is some thousands of pixels across, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let head = [frame.width as f32, frame.height as f32, placing.at.x, placing.at.y];
    let tail = [placing.opacity, depth, rim_width, SPAN];

    bytes(head.into_iter().chain(clip).chain(rim.channels()).chain(tail))
}

/// The bytes of `values`, in the platform's order, as the GPU reads them: as many as the array holds, the rest zero.
fn bytes<const N: usize>(values: impl IntoIterator<Item = f32>) -> [u8; N] {
    let mut bytes = [0; N];
    for (at, value) in (0..N).step_by(4).zip(values) {
        bytes[at..at + 4].copy_from_slice(&value.to_ne_bytes());
    }

    bytes
}
