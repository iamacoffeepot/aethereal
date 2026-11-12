//! What the renderer tells of a frame (ADR-0053), on the device that draws nothing: each pass named in the order the
//! passes write their timestamps, the pass the renderer adds after a window's main frame's last among them, each draw counted only into a frame that counts, and the totals of what it made
//! kept as a walk over all of it would count them.

use super::lists::{placed, tinted};
use super::*;

/// A frame onto a texture 4 by 4 of `Rgba8`, of one pass, [`Main`], drawing with no depth.
fn onto_texture() -> CreateFrame {
    let main = PassSpec::of::<Main>(Does::Draw {
        into: Target::End,
        depth: DepthUse::Off,
        load: Load::Clear,
        reads: Vec::new(),
    });

    CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: Vec::new(),
        passes: vec![main],
    }
}

#[test]
fn passes_are_named_in_the_order_they_write_their_timestamps() {
    // Catches a pass's time told under another pass's name: names listed with the window's main frame before the
    // frames that end on textures, which run first, or a frame's passes out of their order, or more or fewer names
    // than the passes that write timestamps.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);
    let lay = laid(&device, &mut kept);
    assert!(matches!(kept.frame(&device, rims_frame(lay, vec![Target::Texture(1)]), MAKER), Ok(Framed::Made(_))));
    let Ok(Framed::Texture { frame, .. }) = kept.frame(&device, onto_texture(), MAKER) else {
        panic!("the frame onto a texture is made");
    };
    let windows = surface(&device);

    let names = kept.pass_names(&windows);

    let main = Frame::Main(window());
    let ran = |frame, pass, label: &str| Ran { frame, pass, label: String::from(label) };
    let expected = [
        ran(Frame::Texture(frame), PassName::of::<Main>(), "aether.render.main"),
        ran(main, PassName::of::<Main>(), "aether.render.main"),
        ran(main, PassName::of::<Rims>(), "aether.test.rims"),
        ran(main, PassName::of::<Composite>(), "aether.test.composite"),
        ran(main, PassName::of::<Interface>(), "aether.render.interface"),
    ];
    assert_eq!(names, expected);
    assert_eq!(names.len(), kept.passes(&windows) as usize);
}

#[test]
fn a_frame_that_counts_counts_each_draw_and_its_triangles_and_one_that_does_not_counts_nothing() {
    // Catches a draw left uncounted or counted twice, a quad of two triangles counted as its six indices, and a frame
    // recorded while no one watches that counts anyway.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);
    for _ in 0..2 {
        placed(&gpu, &mut kept, (MAKER, program, placement::Order::Loose), (-1.0, 1.0), [0.5; 4]);
    }
    let windows = surface(&device);
    let mut counting = Tally::On(stats::FrameDrawn::default());
    let mut not = Tally::Off;

    for tally in [&mut counting, &mut not] {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        kept.run_all(&mut encoder, &windows, None, tally);
        queue.submit([encoder.finish()]);
    }

    assert_eq!(counting, Tally::On(stats::FrameDrawn { draws: 2, triangles: 4, uploaded_bytes: 0 }));
    assert_eq!(not, Tally::Off);
}

/// What `kept` holds of the GPU's memory and how many of each thing are alive, each counted by walking all it holds:
/// what its totals answer, kept right.
fn walked(kept: &Kept) -> (GpuMemory, stats::Alive) {
    let of = |origin: fn(&Origin) -> bool| -> u64 {
        let textures = kept.textures.values().filter(|texture| origin(&texture.origin));
        textures.map(|texture| stats::texture_bytes(&texture.made)).sum()
    };
    let frames: u64 = kept.frames.values().map(Walk::targets_bytes_counted).sum();
    let windows: u64 =
        kept.windows.iter().map(|windowed| windowed.own.targets_bytes_counted() + windowed.last.bytes()).sum();
    let instances: u64 = kept.instances.values().map(|made| made.buffer.size()).sum();
    let uniforms: u64 = kept.uniforms.values().map(|made| made.buffer.size()).sum();
    let storage: u64 = kept.storage.values().map(|made| made.buffer.size()).sum();
    let memory = GpuMemory {
        meshes_bytes: kept.meshes.values().map(|mesh| mesh.vertices.size() + mesh.indices.size()).sum(),
        textures_bytes: of(|origin| matches!(origin, Origin::Pixels)),
        targets_bytes: frames + windows + of(|origin| matches!(origin, Origin::Frame(_))),
        buffers_bytes: instances + uniforms + storage,
        atlas_bytes: kept.text.atlas().1,
    };

    let count = |alive: usize| u32::try_from(alive).unwrap_or(u32::MAX);
    let alive = stats::Alive {
        meshes: count(kept.meshes.values().count()),
        textures: count(kept.textures.values().count()),
        programs: count(kept.programs.values().count()),
        placements: count(kept.placements.values().count() + kept.text.placed.values().count()),
        texts: count(kept.text.made.values().count()),
    };

    (memory, alive)
}

/// The GPU memory and the counts alive `kept` tells a frame's subscribers of.
fn totals(kept: &Kept) -> (GpuMemory, stats::Alive) {
    let told = kept.frame_stats(0, stats::FrameBudget::UNTOLD, FrameDrawing::Undrawn);

    (told.memory, told.alive)
}

/// A frame that follows the window at half its width and its whole height, ending on a texture of `Rgba8`, drawing
/// with a depth of its own.
fn following() -> CreateFrame {
    let main = PassSpec::of::<Main>(Does::Draw {
        into: Target::End,
        depth: DepthUse::Write(0),
        load: Load::Clear,
        reads: Vec::new(),
    });

    CreateFrame {
        ends: Ends::Follows { window: window(), width: Scale::Half, height: Scale::Full, format: TextureFormat::Rgba8 },
        targets: vec![TargetSpec { kind: TargetKind::Depth, scale: Scale::Full, samples: Samples::One }],
        passes: vec![main],
    }
}

/// Keeps a frame `width` by `height` drawn onto the window as its last, as each `Render` does once it has drawn.
fn last_kept(device: &wgpu::Device, queue: &wgpu::Queue, kept: &mut Kept, (width, height): (u32, u32)) {
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
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.keep_last(device, &mut encoder, &[(window(), frame)]);
    queue.submit([encoder.finish()]);
}

#[test]
fn the_totals_kept_as_things_are_made_resized_and_released_are_what_a_walk_counts() {
    // Catches a total left counting what was released or forgotten, a frame's targets or a view's texture made again
    // at its window's new size and counted at the old, a window's last frame or own frame left out as it is kept,
    // fitted, or closed, and a texture a frame ends on counted as a texture of pixels.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);
    for left in [-1.0, 0.0] {
        placed(&gpu, &mut kept, (MAKER, program, placement::Order::Loose), (left, 1.0), [0.5; 4]);
    }
    let pixels = kept.ids.next();
    let (made, view) = end_texture(&device, Size { width: 3, height: 5 }, TextureFormat::Rgba8);
    kept.textures.keep(pixels, MAKER, Texture { made, view, format: TextureFormat::Rgba8, origin: Origin::Pixels });
    let stored = kept.ids.next();
    kept.storage.keep(stored, MAKER, Storage { buffer: upload(&gpu, &[0; 256], STORAGE_USAGE), size: 256 });
    let lay = laid(&device, &mut kept);
    assert!(matches!(kept.frame(&device, rims_frame(lay, vec![Target::Texture(1)]), MAKER), Ok(Framed::Made(_))));
    assert_eq!(frame::check(&following(), &kept), Ok(()));
    let Ok(Framed::Texture { frame: followed, .. }) = kept.frame(&device, following(), MAKER) else {
        panic!("the frame that follows the window is made");
    };

    let (made, alive) = totals(&kept);
    assert_eq!((made, alive), walked(&kept), "as made");
    let each = [made.meshes_bytes, made.textures_bytes, made.targets_bytes, made.buffers_bytes, made.atlas_bytes];
    assert!(each.iter().all(|bytes| *bytes > 0), "each kind holds memory: {made:?}");

    drawn_at(&device, &queue, &mut kept, (8, 6));
    last_kept(&device, &queue, &mut kept, (8, 6));
    assert_eq!(totals(&kept), walked(&kept), "drawn at 8 by 6");

    drawn_at(&device, &queue, &mut kept, (5, 7));
    last_kept(&device, &queue, &mut kept, (5, 7));
    assert_eq!(totals(&kept), walked(&kept), "resized to 5 by 7");

    let released = [Release::Texture(TextureId { id: pixels }), Release::Storage(StorageId { id: stored })];
    for release in released.into_iter().chain([Release::Frame(followed)]) {
        assert_eq!(kept.release(release, MAKER), Owned::Mine(()), "{release:?} is released");
    }
    assert_eq!(totals(&kept), walked(&kept), "released");

    assert!(kept.closed(window()));
    assert_eq!(totals(&kept), walked(&kept), "its window closed");
}

#[test]
fn an_actor_s_end_takes_what_it_made_out_of_the_totals() {
    // Catches an ending that releases what its actor made but leaves it counted, so the totals grow with each actor
    // that comes and goes.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let before = totals(&kept);
    let program = tinted(&gpu, &mut kept);
    placed(&gpu, &mut kept, (MAKER, program, placement::Order::Loose), (-1.0, 1.0), [0.5; 4]);
    let lay = laid(&device, &mut kept);
    assert!(matches!(kept.frame(&device, rims_frame(lay, vec![Target::Texture(1)]), MAKER), Ok(Framed::Made(_))));
    assert!(matches!(kept.frame(&device, following(), MAKER), Ok(Framed::Texture { .. })));
    drawn_at(&device, &queue, &mut kept, (8, 6));
    assert_ne!(totals(&kept), before, "what it made is counted");

    kept.ended(MAKER);

    assert_eq!(totals(&kept), before);
    assert_eq!(totals(&kept), walked(&kept));
}
