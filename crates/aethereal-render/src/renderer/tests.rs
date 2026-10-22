//! What the renderer makes, made and drawn on a device that draws nothing but checks all it is handed, as WebGPU
//! does.

use super::*;
use crate::asked::blob;
use crate::frame::{PassSpec, Scale, TargetKind, TargetSpec};
use crate::pass::{Main, Pass};
use crate::placement::Resource;
use aethereal_data::{Budget, from_bytes};

/// The actor that makes everything here.
const MAKER: ActorId = ActorId::new(1);

/// An effect that samples the target it reads, smoothly, by the place across it the renderer's vertex stage writes.
const SMOOTHED: &str = "
@group(0) @binding(0) var glow: texture_2d<f32>;
@group(1) @binding(0) var smoothly: sampler;

@fragment
fn composite(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(glow, smoothly, uv);
}
";

/// The pass the effect draws in.
#[derive(Plain)]
#[kind(name = "aether.test.composite")]
struct Composite;

impl Pass for Composite {}

/// The window opened first, as a host's window capability numbers it.
fn window() -> WindowId {
    let Ok(window) = from_bytes(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// The renderer's kept state, on `device`, before anything is asked of it, drawing into a window of `Bgra8`.
fn kept(device: &wgpu::Device) -> Kept {
    let format = ColourFormat::Bgra8;
    let Ok(own) = Walk::new(device, CreateFrame::own(window()), (End::Window, format), Size::PIXEL, effectless) else {
        panic!("the renderer's own frame is made");
    };

    Kept::new(device, (window(), format), own)
}

/// A texture of the window's format, drawn into as the window's surface would be, of the one pixel the renderer's own
/// frame is made at before the first frame's size.
fn surface(device: &wgpu::Device) -> wgpu::TextureView {
    surface_of(device, 1, 1)
}

/// A texture of the window's format, drawn into as the window's surface would be, `width` by `height`.
fn surface_of(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ColourFormat::Bgra8.wgpu(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

#[test]
fn an_effect_binding_a_sampler_none_has_made_is_made_and_drawn() {
    // Catches an effect pass that only looks up the samplers it binds: the frame was refused as binding what is
    // missing unless a placement or a dispatch had made that same sampler first.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);

    let create = CreateEffect {
        wgsl: blob(),
        fragment: String::from("composite"),
        reads: vec![Read::Float],
        bindings: vec![Binding { group: 1, binding: 0, kind: BindingKind::Sampler }],
        blend: Blend::Opaque,
        shape: Shape::flat(ColourFormat::Rgba8),
    };
    assert_eq!(effect::check(&create, SMOOTHED, &device.limits()), Ok(()));
    let program = kept.ids.next();
    kept.programs.keep(program, MAKER, effect_pipeline(&device, &create, SMOOTHED));

    let smooth = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };
    let glow = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::Texture(0), depth: DepthUse::Off, load: Load::Clear },
    };
    let composite = PassSpec {
        name: PassName::of::<Composite>(),
        does: Does::Effect {
            program: ProgramId { id: program },
            reads: vec![Target::Texture(0)],
            into: Target::End,
            bindings: vec![Bind { group: 1, binding: 0, to: Resource::Sampler(smooth) }],
        },
    };
    let frame = CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: vec![TargetSpec { kind: TargetKind::Float, scale: Scale::Full }],
        passes: vec![glow, composite],
    };
    assert_eq!(frame::check(&frame, &kept), Ok(()));

    let framed = kept.frame(&device, frame, MAKER);
    assert!(matches!(framed, Ok(Framed::Texture { .. })), "the frame is refused: {framed:?}");

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.run_all(&mut encoder, &surface(&device), None);
    queue.submit([encoder.finish()]);
}

/// Fits the window's main frame of `kept` to a surface `width` by `height`, as each `Render` does before it draws, and
/// draws it there.
fn drawn_at(device: &wgpu::Device, queue: &wgpu::Queue, kept: &mut Kept, (width, height): (u32, u32)) {
    kept.main_mut().fit(device, Size { width, height });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.run_all(&mut encoder, &surface_of(device, width, height), None);
    queue.submit([encoder.finish()]);
}

#[test]
fn a_frame_that_ends_on_the_window_follows_its_surface_to_each_new_size() {
    // Catches a frame's targets left at the size they were made, or at the size the frame before it was fitted to,
    // when its window is drawn at another: its depth is then another size than the surface it is drawn with, which
    // WebGPU refuses, and the frame is not drawn.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);
    drawn_at(&device, &queue, &mut kept, (8, 6));

    let onto = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear },
    };
    let frame = CreateFrame {
        ends: Ends::Window(window()),
        targets: vec![TargetSpec { kind: TargetKind::Depth, scale: Scale::Full }],
        passes: vec![onto],
    };
    assert_eq!(frame::check(&frame, &kept), Ok(()));
    let framed = kept.frame(&device, frame, MAKER);
    assert!(matches!(framed, Ok(Framed::Made(_))), "the frame is refused: {framed:?}");

    drawn_at(&device, &queue, &mut kept, (8, 6));
    drawn_at(&device, &queue, &mut kept, (5, 7));
}

/// An effect that reads no target and shades the place across its target by uniforms, as a sky does.
const SKY: &str = "
@group(1) @binding(0) var<uniform> horizon: vec4<f32>;

@fragment
fn sky(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return horizon * uv.y;
}
";

/// The pass the sky draws in.
#[derive(Plain)]
#[kind(name = "aether.test.sky")]
struct Sky;

impl Pass for Sky {}

#[test]
fn a_frame_begun_by_an_effect_that_reads_nothing_is_made_and_drawn_from_black() {
    // Catches an effect that reads nothing made without its empty group 0, its uniforms bound where its shader does
    // not look for them, which WebGPU refuses as the pipeline is made or the pass draws, and the first pass of a
    // frame begun from what its target held before rather than from black.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);

    let create = CreateEffect {
        wgsl: blob(),
        fragment: String::from("sky"),
        reads: Vec::new(),
        bindings: vec![Binding { group: 1, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
        blend: Blend::Opaque,
        shape: Shape::flat(ColourFormat::Rgba8),
    };
    assert_eq!(effect::check(&create, SKY, &device.limits()), Ok(()));
    let program = kept.ids.next();
    kept.programs.keep(program, MAKER, effect_pipeline(&device, &create, SKY));
    let horizon = kept.ids.next();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: false,
    });
    kept.uniforms.keep(horizon, MAKER, Uniforms { buffer, size: 16 });

    let sky = PassSpec {
        name: PassName::of::<Sky>(),
        does: Does::Effect {
            program: ProgramId { id: program },
            reads: Vec::new(),
            into: Target::End,
            bindings: vec![Bind { group: 1, binding: 0, to: Resource::Uniforms(UniformsId { id: horizon }) }],
        },
    };
    let frame = CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: Vec::new(),
        passes: vec![sky],
    };
    assert_eq!(frame::check(&frame, &kept), Ok(()));

    let framed = kept.frame(&device, frame, MAKER);
    let Ok(Framed::Texture { frame, .. }) = framed else {
        panic!("the frame is refused: {framed:?}");
    };
    let Some(walk) = kept.frames.get(frame.id) else {
        panic!("the frame is kept");
    };
    let loads: Vec<Load> = walk
        .steps()
        .filter_map(|(_, step)| match step {
            Step::Effect { effect, .. } => Some(effect.load),
            Step::Draw { .. } | Step::Compute => None,
        })
        .collect();
    assert_eq!(loads, vec![Load::Clear]);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.run_all(&mut encoder, &surface(&device), None);
    queue.submit([encoder.finish()]);
}

/// An effect that reads nothing and writes a glow past one, as a bloom kept between frames does.
const GLOW: &str = "
@fragment
fn glow(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(2.5, 2.5, 2.5, 1.0);
}
";

/// An effect that samples a texture another frame ends on, smoothly, and darkens it by a quarter.
const DARKENED: &str = "
@group(1) @binding(0) var glow: texture_2d<f32>;
@group(1) @binding(1) var smoothly: sampler;

@fragment
fn composite(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(glow, smoothly, uv) / 4.0;
}
";

/// The effect `fragment` of `wgsl`, reading no target of its frame, binding `bindings`, made for `shape` and kept
/// in `kept`.
fn effect_of(
    device: &wgpu::Device,
    kept: &mut Kept,
    (wgsl, fragment): (&str, &str),
    bindings: Vec<Binding>,
    shape: Shape,
) -> ProgramId {
    let create = CreateEffect {
        wgsl: blob(),
        fragment: String::from(fragment),
        reads: Vec::new(),
        bindings,
        blend: Blend::Opaque,
        shape,
    };
    assert_eq!(effect::check(&create, wgsl, &device.limits()), Ok(()));
    let program = kept.ids.next();
    kept.programs.keep(program, MAKER, effect_pipeline(device, &create, wgsl));

    ProgramId { id: program }
}

/// A frame onto a texture 4 by 4 of `format`, of the one effect pass `does`.
fn onto(format: TextureFormat, does: Does) -> CreateFrame {
    let pass = PassSpec { name: PassName::of::<Composite>(), does };

    CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format },
        targets: Vec::new(),
        passes: vec![pass],
    }
}

#[test]
fn a_frame_ending_in_floats_is_sampled_by_another_frame_s_effect_and_read_back_in_floats() {
    // Catches a texture end made in bytes whatever format it was asked in, which an effect made for floats is
    // refused to draw into as WebGPU draws it; its passes shaped as bytes, so a program made for them joins none
    // and one made for bytes is drawn into floats; and one kept under another format than it was made in, which is
    // read back as bytes it does not hold.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);

    let glow = effect_of(&device, &mut kept, (GLOW, "glow"), Vec::new(), Shape::flat(ColourFormat::Rgba16Float));
    let glow = Does::Effect { program: glow, reads: Vec::new(), into: Target::End, bindings: Vec::new() };
    let glowing = onto(TextureFormat::Rgba16Float, glow);
    assert_eq!(frame::check(&glowing, &kept), Ok(()));
    let framed = kept.frame(&device, glowing, MAKER);
    let Ok(Framed::Texture { frame, texture }) = framed else {
        panic!("the frame is refused: {framed:?}");
    };
    let shaped = kept.pass(Frame::Texture(frame), PassName::of::<Composite>());
    assert_eq!(shaped, Ok(Shape::flat(ColourFormat::Rgba16Float)));

    let bindings = vec![
        Binding { group: 1, binding: 0, kind: BindingKind::Texture },
        Binding { group: 1, binding: 1, kind: BindingKind::Sampler },
    ];
    let darken = effect_of(&device, &mut kept, (DARKENED, "composite"), bindings, Shape::flat(ColourFormat::Rgba8));
    let smooth = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };
    let darken = Does::Effect {
        program: darken,
        reads: Vec::new(),
        into: Target::End,
        bindings: vec![
            Bind { group: 1, binding: 0, to: Resource::Texture(texture) },
            Bind { group: 1, binding: 1, to: Resource::Sampler(smooth) },
        ],
    };
    let darkened = onto(TextureFormat::Rgba8, darken);
    assert_eq!(frame::check(&darkened, &kept), Ok(()));
    let framed = kept.frame(&device, darkened, MAKER);
    assert!(matches!(framed, Ok(Framed::Texture { .. })), "the frame is refused: {framed:?}");

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.run_all(&mut encoder, &surface(&device), None);
    let Some(glowed) = kept.textures.get(texture.id) else {
        panic!("the texture the frame ends on is kept");
    };
    let reading = copy_texture(&device, &mut encoder, &glowed.made, glowed.format, Order::Kept);
    queue.submit([encoder.finish()]);

    let Reading::Pixels { copied, format, .. } = reading else {
        panic!("a texture is read back as pixels");
    };
    assert_eq!((format, copied.pixel), (TextureFormat::Rgba16Float, 8));
}

/// An effect that reads a target of floats at the pixel it shades, as a downsample of one of another size would not.
const LOADED: &str = "
@group(0) @binding(0) var scene: texture_2d<f32>;

@fragment
fn composite(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(scene, vec2<i32>(at.xy), 0);
}
";

/// The pass that downsamples the scene into a quarter-size target.
#[derive(Plain)]
#[kind(name = "aether.test.downsample")]
struct Downsample;

impl Pass for Downsample {}

/// The sides of each target of the window's main frame of `kept`, in order.
fn sides(kept: &Kept, targets: u32) -> Vec<(u32, u32)> {
    (0..targets).map(|at| kept.main().target(at).texture()).map(|texture| (texture.width(), texture.height())).collect()
}

#[test]
fn a_frame_of_half_and_quarter_size_targets_is_drawn_at_each_size_its_window_is_drawn_at() {
    // Catches a target made at the end's size whatever its scale, or at the scale's size rounded down, and one made
    // again at the end's size when the window is resized; and a draw into a half-size target whose depth is of
    // another size, or an effect over a quarter-size target drawn at the end's size, which WebGPU refuses; and the
    // targets made again at every `Render`, though the window kept its size.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);

    let reading = |kept: &mut Kept, shape| {
        let create = CreateEffect {
            wgsl: blob(),
            fragment: String::from("composite"),
            reads: vec![Read::Float],
            bindings: Vec::new(),
            blend: Blend::Opaque,
            shape,
        };
        assert_eq!(effect::check(&create, LOADED, &device.limits()), Ok(()));
        let program = kept.ids.next();
        kept.programs.keep(program, MAKER, effect_pipeline(&device, &create, LOADED));
        ProgramId { id: program }
    };
    let downsample = reading(&mut kept, Shape::flat(ColourFormat::Rgba16Float));
    let composite = reading(&mut kept, Shape::flat(ColourFormat::Bgra8));

    let scene = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(1), load: Load::Clear },
    };
    let downsampled = PassSpec {
        name: PassName::of::<Downsample>(),
        does: Does::Effect {
            program: downsample,
            reads: vec![Target::Texture(0)],
            into: Target::Texture(2),
            bindings: Vec::new(),
        },
    };
    let composited = PassSpec {
        name: PassName::of::<Composite>(),
        does: Does::Effect {
            program: composite,
            reads: vec![Target::Texture(2)],
            into: Target::End,
            bindings: Vec::new(),
        },
    };
    let frame = CreateFrame {
        ends: Ends::Window(window()),
        targets: vec![
            TargetSpec { kind: TargetKind::Float, scale: Scale::Half },
            TargetSpec { kind: TargetKind::Depth, scale: Scale::Half },
            TargetSpec { kind: TargetKind::Float, scale: Scale::Quarter },
        ],
        passes: vec![scene, downsampled, composited],
    };
    assert_eq!(frame::check(&frame, &kept), Ok(()));
    let framed = kept.frame(&device, frame, MAKER);
    assert!(matches!(framed, Ok(Framed::Made(_))), "the frame is refused: {framed:?}");

    drawn_at(&device, &queue, &mut kept, (9, 6));
    assert_eq!(sides(&kept, 3), vec![(5, 3), (5, 3), (3, 2)]);
    drawn_at(&device, &queue, &mut kept, (40, 3));
    assert_eq!(sides(&kept, 3), vec![(20, 2), (20, 2), (10, 1)]);

    let fitted = kept.main().target(2).clone();
    drawn_at(&device, &queue, &mut kept, (40, 3));
    assert_eq!(kept.main().target(2), &fitted, "a frame drawn again at its size keeps its targets");
}
