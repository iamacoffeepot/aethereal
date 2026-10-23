//! Draw lists as the renderer keeps and draws them (ADR-0031): on the device that draws nothing, which placements a
//! pass draws and in what order; and, where the machine has a GPU adapter, the pixels two overlapping blended
//! placements draw in the order listed.

// The GPU test says which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::*;
use crate::draw_list::DrawList;

/// An actor other than [`MAKER`].
const OTHER: ActorId = ActorId::new(2);

/// A program that draws a quad's corners in the colour its uniforms hold, laid over what is drawn by its alpha.
const TINTED: &str = "
@group(0) @binding(0) var<uniform> tint: vec4<f32>;

@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.5, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return tint;
}
";

/// The program [`TINTED`], blending by alpha, testing depth and writing none, for the renderer's own main frame's
/// pass [`Main`], kept in `kept`.
fn tinted(gpu: &Gpu, kept: &mut Kept) -> ProgramId {
    let create = CreateProgram {
        wgsl: blob(),
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
        reads: Vec::new(),
        state: Fixed { blend: Blend::Alpha, depth: Depth::Test, cull: Cull::None, coverage: Coverage::Off },
        shape: Shape::with_depth(ColourFormat::Bgra8),
    };
    assert_eq!(program::check(&create, TINTED, &gpu.device().limits()), Ok(()));
    let program = kept.ids.next();
    kept.programs.keep(program, MAKER, pipeline(gpu, &create, TINTED));

    ProgramId { id: program }
}

/// A placement by `maker` of a quad from `left` to `right` across the window, top to bottom, tinted `tint` by
/// `program`, joining the window's main frame's pass [`Main`] as `order` says, each made and kept in `kept`. Answers
/// its id.
fn placed(
    gpu: &Gpu,
    kept: &mut Kept,
    (maker, program, order): (ActorId, ProgramId, placement::Order),
    (left, right): (f32, f32),
    tint: [f32; 4],
) -> PlacementId {
    let vertices: Vec<u8> =
        [left, -1.0, right, -1.0, right, 1.0, left, 1.0].iter().flat_map(|value| value.to_ne_bytes()).collect();
    let indices: Vec<u8> = [0_u32, 1, 2, 0, 2, 3].iter().flat_map(|value| value.to_ne_bytes()).collect();
    let mesh = Mesh {
        vertices: upload(gpu, &vertices, wgpu::BufferUsages::VERTEX),
        indices: upload(gpu, &indices, wgpu::BufferUsages::INDEX),
        counted: Counted { vertices: 4, indices: 6 },
        layout: corners(),
    };
    let mesh_id = kept.ids.next();
    kept.meshes.keep(mesh_id, maker, mesh);
    let bytes: Vec<u8> = tint.iter().flat_map(|value| value.to_ne_bytes()).collect();
    let uniforms = kept.ids.next();
    kept.uniforms.keep(
        uniforms,
        maker,
        Uniforms { buffer: upload(gpu, &bytes, wgpu::BufferUsages::UNIFORM), size: 16 },
    );

    let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(UniformsId { id: uniforms }) }];
    let frame = Frame::Main(window());
    let join = match order {
        placement::Order::Loose => Join::to::<Main>(frame, program, bindings),
        placement::Order::Listed => Join::listed::<Main>(frame, program, bindings),
    };
    let place = Place { mesh: MeshId { id: mesh_id }, instances: Drawn::Count(1), joins: vec![join] };
    assert_eq!(placement::check(&place, &*kept), Ok(()));
    let made = kept.placement(gpu.device(), &place);
    let Ok(made) = made else {
        panic!("the placement is refused: {:?}", made.err());
    };
    let id = kept.ids.next();
    kept.placements.keep(id, maker, made);

    PlacementId { id }
}

/// A placement by [`MAKER`] of a quad over the whole window, tinted grey by `program`, joining [`Main`] as `order`
/// says.
fn whole(gpu: &Gpu, kept: &mut Kept, program: ProgramId, order: placement::Order) -> PlacementId {
    placed(gpu, kept, (MAKER, program, order), (-1.0, 1.0), [0.5, 0.5, 0.5, 0.5])
}

/// A list of `placements` in the window's main frame's pass [`Main`].
fn main_list(placements: &[PlacementId]) -> DrawList {
    DrawList::of::<Main>(Frame::Main(window()), placements.to_vec())
}

/// The placements the window's main frame's pass [`Main`] draws, in order.
fn drawn_in_main(kept: &Kept) -> Vec<u64> {
    let Ok(fits) = kept.pass(Frame::Main(window()), PassName::of::<Main>()) else {
        panic!("the window's main frame has the pass");
    };

    kept.drawn((Frame::Main(window()), PassName::of::<Main>()), &fits).into_iter().map(|(id, ..)| id).collect()
}

/// A device that draws nothing, the renderer's kept state on it, a program for [`Main`], and the GPU it is opened as.
fn nothing_drawn() -> (Gpu, Kept, ProgramId) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);

    (gpu, kept, program)
}

#[test]
fn a_listed_placement_draws_only_while_a_list_holds_it_after_the_loose_in_the_lists_order() {
    // Catches a placement joined listed drawn loose before any list names it, or still drawn once a newer list leaves
    // it out; an empty list that keeps the last list drawing; and a list drawn in the order of its placements' ids, or
    // a hash of them, rather than its own.
    let (gpu, mut kept, program) = nothing_drawn();
    let loose = whole(&gpu, &mut kept, program, placement::Order::Loose);
    let [a, b, c] = [(); 3].map(|()| whole(&gpu, &mut kept, program, placement::Order::Listed));
    assert_eq!(drawn_in_main(&kept), vec![loose.id], "no list names the listed ones");

    assert_eq!(kept.listed(&main_list(&[c, a, b]), MAKER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![loose.id, c.id, a.id, b.id]);
    assert_eq!(kept.listed(&main_list(&[b, c]), MAKER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![loose.id, b.id, c.id], "a culled by being left out");
    assert_eq!(kept.listed(&main_list(&[]), MAKER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![loose.id], "an empty list draws none");
}

#[test]
fn a_list_refused_leaves_the_last_drawing_and_a_listed_placement_released_leaves_it_in_the_release() {
    // Catches a list refused for naming a placement another made, or a frame that went, that still replaces or blanks
    // the last; and a released placement left in its maker's list, drawn after its release or found missing at
    // `Render`, or another's release that takes it out.
    let (gpu, mut kept, program) = nothing_drawn();
    let [a, b, c] = [(); 3].map(|()| whole(&gpu, &mut kept, program, placement::Order::Listed));
    let theirs = placed(&gpu, &mut kept, (OTHER, program, placement::Order::Listed), (-1.0, 1.0), [0.0; 4]);
    assert_eq!(kept.listed(&main_list(&[a, b, c]), MAKER), Ok(()));

    let not_owned = Err(ListRefused::Entry { at: 1, why: EntryRefused::NotOwner });
    assert_eq!(kept.listed(&main_list(&[c, theirs]), MAKER), not_owned);
    let gone = DrawList::of::<Main>(Frame::Texture(FrameId { id: 999 }), vec![a]);
    assert_eq!(kept.listed(&gone, MAKER), Err(ListRefused::Frame));
    assert_eq!(drawn_in_main(&kept), vec![a.id, b.id, c.id]);

    assert_eq!(kept.release_placement(b.id, OTHER), Owned::NotOwner);
    assert_eq!(drawn_in_main(&kept), vec![a.id, b.id, c.id]);
    assert_eq!(kept.release_placement(b.id, MAKER), Owned::Mine(()));
    assert_eq!(drawn_in_main(&kept), vec![a.id, c.id]);
    let named_released = Err(ListRefused::Entry { at: 0, why: EntryRefused::Placement });
    assert_eq!(kept.listed(&main_list(&[b]), MAKER), named_released);
}

#[test]
fn an_actor_that_ends_takes_its_lists_and_another_actors_list_draws_on() {
    // Catches an ended actor's lists kept, naming placements that went with it, and another actor's dropped with them.
    let (gpu, mut kept, program) = nothing_drawn();
    let mine = whole(&gpu, &mut kept, program, placement::Order::Listed);
    let theirs = placed(&gpu, &mut kept, (OTHER, program, placement::Order::Listed), (-1.0, 1.0), [0.0; 4]);
    assert_eq!(kept.listed(&main_list(&[mine]), MAKER), Ok(()));
    assert_eq!(kept.listed(&main_list(&[theirs]), OTHER), Ok(()));

    kept.ended(MAKER);
    assert_eq!(drawn_in_main(&kept), vec![theirs.id]);
    assert_eq!(kept.lists.of(Frame::Main(window()), PassName::of::<Main>()).count(), 1);
}

/// A frame onto a texture 4 by 4 of `Rgba8`, of one pass, [`Main`], drawing with depth.
fn onto_texture() -> CreateFrame {
    let main = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear, reads: Vec::new() },
    };

    CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: vec![TargetSpec { kind: TargetKind::Depth, scale: Scale::Full, samples: Samples::One }],
        passes: vec![main],
    }
}

#[test]
fn a_texture_frame_released_takes_its_lists_and_a_main_frame_replaced_keeps_them() {
    // Catches lists for a frame that ended on a texture kept once it is released, though no list names it again; and
    // a window's lists dropped, or left with the frame replaced, when an actor makes the window's main frame.
    let (gpu, mut kept, program) = nothing_drawn();
    let Ok(Framed::Texture { frame, .. }) = kept.frame(gpu.device(), onto_texture(), MAKER) else {
        panic!("the frame is made");
    };
    let listed = whole(&gpu, &mut kept, program, placement::Order::Listed);
    let texture_list = DrawList::of::<Main>(Frame::Texture(frame), Vec::new());
    assert_eq!(kept.listed(&texture_list, MAKER), Ok(()));
    assert_eq!(kept.listed(&main_list(&[listed]), MAKER), Ok(()));

    let released = kept.frames.release(frame.id, MAKER);
    let Owned::Mine(walk) = released else {
        panic!("the frame is released");
    };
    kept.unframed([(frame.id, walk.texture())]);
    assert_eq!(kept.lists.of(Frame::Texture(frame), PassName::of::<Main>()).count(), 0);
    assert_eq!(kept.listed(&texture_list, MAKER), Err(ListRefused::Frame));

    let main = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear, reads: Vec::new() },
    };
    let depth = TargetSpec { kind: TargetKind::Depth, scale: Scale::Full, samples: Samples::One };
    let replacing = CreateFrame { ends: Ends::Window(window()), targets: vec![depth], passes: vec![main] };
    assert!(matches!(kept.frame(gpu.device(), replacing, MAKER), Ok(Framed::Made(_))));
    assert_eq!(drawn_in_main(&kept), vec![listed.id], "the list follows the main frame");
}

/// A device on the machine's adapter, if it has one, with the adapter's name.
fn adapter_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let mut described = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    described.backends &= wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(described);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|error| error.to_string())?;
    let name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|error| error.to_string())?;

    Ok((device, queue, name))
}

/// The pixel at the middle of the window's 4 by 4 frame, red, green, blue, and alpha, once the renderer's kept state
/// on `gpu` draws two half-transparent quads, a red one over the left three quarters and a blue one over the right
/// three quarters, listed in `order`, after `unrelated` ids are taken as making as many unrelated things would.
fn overlap(gpu: &Gpu, unrelated: u32, order: [usize; 2]) -> [u8; 4] {
    let device = gpu.device();
    let mut kept = kept(device);
    for _ in 0..unrelated {
        kept.ids.next();
    }
    let program = tinted(gpu, &mut kept);
    let listed = (MAKER, program, placement::Order::Listed);
    let red = placed(gpu, &mut kept, listed, (-1.0, 0.5), [1.0, 0.0, 0.0, 0.5]);
    let blue = placed(gpu, &mut kept, listed, (-0.5, 1.0), [0.0, 0.0, 1.0, 0.5]);
    let quads = [red, blue];
    assert_eq!(kept.listed(&main_list(&order.map(|at| quads[at])), MAKER), Ok(()));

    let (width, height) = (4, 4);
    let frame = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ColourFormat::Bgra8.wgpu(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    kept.fit(device, window(), Size { width, height });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let windows = [(window(), frame.create_view(&wgpu::TextureViewDescriptor::default()))];
    kept.run_all(&mut encoder, &windows, None);
    let Ok(Reading::Pixels { buffer, copied, order, .. }) = copy_frame(device, &mut encoder, &frame) else {
        panic!("the frame is copied");
    };
    gpu.queue().submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the copy maps"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
    let Some(pixels) = mapped_bytes(&buffer, |bytes| copied.unpad(bytes, order)) else {
        panic!("the copy is read");
    };

    // The second row's second pixel: inside both quads.
    let at = (width as usize + 1) * 4;
    let Ok(pixel) = <[u8; 4]>::try_from(&pixels[at..at + 4]) else {
        panic!("a pixel is four bytes");
    };

    pixel
}

// The fault #294 found, now declared: two blended placements of one program in one pass, overlapping, give the same
// pixels where they overlap however many unrelated things were made before them, as their maker's list orders them
// and nothing taken from their ids does; and the list reversed draws the other on top. A machine with no adapter, as
// CI's, skips it and says so.
#[test]
fn on_a_gpu_two_overlapping_blended_placements_draw_in_their_list_s_order_whatever_was_made_before() {
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to draw the list on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let gpu = Gpu::new(device, queue);

    let blue_on_top: Vec<[u8; 4]> = (0..8).map(|unrelated| overlap(&gpu, unrelated, [0, 1])).collect();
    let red_on_top: Vec<[u8; 4]> = (0..8).map(|unrelated| overlap(&gpu, unrelated, [1, 0])).collect();

    let [red, _, blue, _] = blue_on_top[0];
    assert!(blue > red, "blue, drawn last, is on top: {:?} on {adapter}", blue_on_top[0]);
    assert!(blue_on_top.iter().all(|pixel| *pixel == blue_on_top[0]), "{blue_on_top:?} on {adapter}");
    let [red, _, blue, _] = red_on_top[0];
    assert!(red > blue, "red, drawn last, is on top: {:?} on {adapter}", red_on_top[0]);
    assert!(red_on_top.iter().all(|pixel| *pixel == red_on_top[0]), "{red_on_top:?} on {adapter}");
}
