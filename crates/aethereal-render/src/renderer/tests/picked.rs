//! Picking (ADR-0055): on the device that draws nothing, the number each draw's call is given in the order the lists
//! are walked, and the stacks answered from what the GPU wrote; and, where the machine has a GPU adapter, the stack
//! the step writes at a probe under overlapping placements.

// The GPU test says which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::lists::{adapter_device, drawn, placed, tinted};
use super::*;
use crate::picking::{MOST_PROBES, STACK};
use aethereal_actor::{ProtocolRef, SeatNumber, Seated};
use aethereal_math::Vec2;
use aethereal_window::{Pressed, Reach};

/// The entry `entry` of [`MAKER`]'s list, which blocks the pointer as `blocks` says.
fn reach(entry: u32, blocks: bool) -> Reach {
    let actor = ProtocolRef::found(Seated { id: MAKER, seat: SeatNumber::new(1) });

    Reach { actor, entry, blocks }
}

/// What reaches the entry `entry` of [`MAKER`]'s list, blocking the pointer as `blocks` says.
fn reaches(entry: u32, blocks: bool) -> Picks {
    Picks::Reaches(reach(entry, blocks))
}

/// A hit in the window's main frame's pass [`Main`], over `rect`, reaching what `picks` says.
fn hit(rect: Rect2, picks: Picks) -> Listed {
    Listed::Hit { frame: Frame::Main(window()), pass: PassName::of::<Main>(), rect, picks }
}

/// A quad by [`MAKER`] over the whole window, listed, tinted grey.
fn whole(gpu: &Gpu, kept: &mut Kept, program: ProgramId) -> PlacementId {
    placed(gpu, kept, (MAKER, program, placement::Order::Listed), (-1.0, 1.0), [0.5, 0.5, 0.5, 0.5])
}

/// A window 4 by 4 of the window's format, to draw into and copy from.
fn window_frame(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ColourFormat::Bgra8.wgpu(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The window's stacks once `kept` on `gpu` draws a `Render` into `frame` with the pointer heard at `x`, `y`, and a
/// press there too when `pressed`: the commands recorded and submitted, and the stacks copied, not yet read.
fn probed(gpu: &Gpu, kept: &mut Kept, frame: &wgpu::Texture, (x, y, pressed): (f32, f32, bool)) -> Stacks {
    let (device, queue) = (gpu.device(), gpu.queue());
    kept.fit(device, window(), Size { width: frame.width(), height: frame.height() });
    let heard = Probed { window: window(), at: 1, x, y, press: pressed };
    for windowed in &mut kept.windows {
        windowed.probing.heard(&heard);
    }
    let windows = [(window(), frame.create_view(&wgpu::TextureViewDescriptor::default()))];
    let mut picking = kept.picking((device, queue), &windows);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    kept.run_all(&mut encoder, &windows, None, &mut Tally::Off, &mut picking);
    let mut stacks = kept.picked(picking, (device, queue), &mut encoder);
    queue.submit([encoder.finish()]);

    let Some(stacks) = stacks.pop() else {
        panic!("the window is probed");
    };

    stacks
}

/// What a GPU would have left at the probes: at the first, the calls `pointer`, the highest first, and at the second,
/// `press`.
fn written(pointer: &[u32], press: &[u32]) -> Vec<u8> {
    let mut words = vec![0_u32; MOST_PROBES * STACK];
    words[..pointer.len()].copy_from_slice(pointer);
    words[STACK..STACK + press.len()].copy_from_slice(press);

    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[test]
fn calls_are_numbered_in_the_walk_s_order_so_a_list_reordered_reorders_the_stack_and_an_ignored_draw_is_not_there() {
    // Catches calls numbered in an order of their own, by placement id or as the GPU drew them, so the stack's top is
    // not what is seen there; a parent's new list that leaves its children's places in the stack where they were; a
    // draw that ignores the pointer given a number, and so a place in the stack; and a hit over the probe left out, or
    // one elsewhere let in.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);
    let [a, ignored, b] = [(); 3].map(|()| whole(&gpu, &mut kept, program));
    let frame = window_frame(&device);
    let over = Rect2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(4.0, 4.0) };
    let elsewhere = Rect2 { min: Vec2::new(3.0, 3.0), max: Vec2::new(4.0, 4.0) };

    let listed = vec![
        drawn(a, reaches(0, true)),
        drawn(ignored, Picks::Ignores),
        hit(over, reaches(2, false)),
        hit(elsewhere, reaches(3, true)),
        drawn(b, reaches(4, false)),
    ];
    assert_eq!(kept.listed(listed, MAKER), Ok(()));
    let stacks = probed(&gpu, &mut kept, &frame, (1.5, 1.5, false));
    let (_, picked) = stacks.picked(&written(&[4, 1], &[]));

    assert_eq!(picked.pointer, vec![reach(4, false), reach(2, false), reach(0, true)]);

    let reordered = vec![drawn(b, reaches(4, false)), drawn(a, reaches(0, true)), drawn(ignored, Picks::Ignores)];
    assert_eq!(kept.listed(reordered, MAKER), Ok(()));
    let stacks = probed(&gpu, &mut kept, &frame, (1.5, 1.5, false));
    let (_, picked) = stacks.picked(&written(&[2, 1], &[]));

    assert_eq!(picked.pointer, vec![reach(0, true), reach(4, false)]);
}

#[test]
fn a_press_is_answered_by_its_own_number_and_its_presses_go_once() {
    // Catches a press answered without the number of the event it is, so the window cannot find whose press the stack
    // is; and a press probed again in the next frame, as though it were pressed twice.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);
    let a = whole(&gpu, &mut kept, program);
    assert_eq!(kept.listed(vec![drawn(a, reaches(0, true))], MAKER), Ok(()));
    let frame = window_frame(&device);

    let pressed = probed(&gpu, &mut kept, &frame, (1.5, 1.5, true)).picked(&written(&[1], &[1])).1;
    let then = probed(&gpu, &mut kept, &frame, (1.5, 1.5, false)).picked(&written(&[1], &[])).1;

    assert_eq!(pressed.presses, vec![Pressed { at: 1, stack: vec![reach(0, true)] }]);
    assert_eq!(then.presses, vec![]);
}

/// The stack the GPU writes under the pointer at the middle of the window's second row's second pixel, once `kept`
/// on `gpu` draws `count` overlapping quads, each over the whole window, the pointer reaching each but the last,
/// listed in that order, each its own entry.
fn stacked(gpu: &Gpu, count: u32) -> Vec<Reach> {
    let mut kept = kept(gpu.device());
    let program = tinted(gpu, &mut kept);
    let listed = (0..count)
        .map(|entry| {
            let quad = whole(gpu, &mut kept, program);
            let ignored = entry + 1 == count;
            let picks = if ignored {
                Picks::Ignores
            } else {
                reaches(entry, false)
            };
            drawn(quad, picks)
        })
        .collect();
    assert_eq!(kept.listed(listed, MAKER), Ok(()));
    let frame = window_frame(gpu.device());

    let stacks = probed(gpu, &mut kept, &frame, (1.5, 1.5, false));
    stacks.buffer.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the stacks map"));
    gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
    let Some((_, picked)) = mapped_bytes(&stacks.buffer, |bytes| stacks.picked(bytes)) else {
        panic!("the stacks are read");
    };

    picked.pointer
}

// What the step writes on a GPU: under quads drawn over one another, the stack holds the last drawn the pointer
// reaches, the last on top, never more than its slots, and never the one that ignores the pointer, though it is drawn
// last. A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_a_probe_s_stack_holds_the_last_drawn_that_reach_it_on_top_and_no_more_than_its_slots() {
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to pick on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let gpu = Gpu::new(device, queue);

    assert_eq!(stacked(&gpu, 3), vec![reach(1, false), reach(0, false)], "on {adapter}");
    let deep: Vec<Reach> = (2..=6).rev().map(|entry| reach(entry, false)).take(STACK).collect();
    assert_eq!(stacked(&gpu, 8), deep, "on {adapter}");
}
