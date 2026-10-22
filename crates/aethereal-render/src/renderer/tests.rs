//! What the renderer makes, made and drawn on a device that draws nothing but checks all it is handed, as WebGPU
//! does.

use super::*;
use crate::asked::blob;
use crate::frame::{PassSpec, TargetSpec};
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
    let Ok(own) = Walk::new(device, CreateFrame::own(window()), (End::Window, format), (1, 1), effectless) else {
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
        ends: Ends::Texture(Size { width: 4, height: 4 }),
        targets: vec![TargetSpec::Float],
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
    kept.main_mut().fit(device, width, height);
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
    let frame = CreateFrame { ends: Ends::Window(window()), targets: vec![TargetSpec::Depth], passes: vec![onto] };
    assert_eq!(frame::check(&frame, &kept), Ok(()));
    let framed = kept.frame(&device, frame, MAKER);
    assert!(matches!(framed, Ok(Framed::Made(_))), "the frame is refused: {framed:?}");

    drawn_at(&device, &queue, &mut kept, (8, 6));
    drawn_at(&device, &queue, &mut kept, (5, 7));
}
