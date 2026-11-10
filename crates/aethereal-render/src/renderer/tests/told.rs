//! What the renderer tells of a frame (ADR-0053), on the device that draws nothing: each pass named in the order the
//! passes write their timestamps, and each draw counted only into a frame that counts.

use super::lists::{placed, tinted};
use super::*;

/// A frame onto a texture 4 by 4 of `Rgba8`, of one pass, [`Main`], drawing with no depth.
fn onto_texture() -> CreateFrame {
    let main = PassSpec {
        name: PassName::of::<Main>(),
        does: Does::Draw { into: Target::End, depth: DepthUse::Off, load: Load::Clear, reads: Vec::new() },
    };

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
    let expected = [
        (Frame::Texture(frame), PassName::of::<Main>()),
        (main, PassName::of::<Main>()),
        (main, PassName::of::<Rims>()),
        (main, PassName::of::<Composite>()),
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
