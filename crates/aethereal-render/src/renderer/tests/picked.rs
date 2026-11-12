//! Picking (ADR-0055): on the device that draws nothing, the number each draw's call is given in the order the lists
//! are walked, and the stacks answered from what the GPU wrote; and, where the machine has a GPU adapter, the stack
//! the step writes at a probe under overlapping placements, in the order they are drawn and at the depths they are
//! drawn at.

// The GPU test says which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::lists::{adapter_device, drawn, placed, quads, tinted};
use super::*;
use crate::draw_list::Reaching;
use crate::picking::{MOST_PROBES, STACK};
use aethereal_actor::{ProtocolRef, SeatNumber, Seated};
use aethereal_math::Vec2;
use aethereal_window::{Pressed, Reach};

/// A program that draws a quad's corners at the depth its uniforms' last holds, opaque in the colour of the rest.
const DEEP: &str = "
@group(0) @binding(0) var<uniform> tint: vec4<f32>;

@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, tint.w, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(tint.rgb, 1.0);
}
";

/// What reaches the entry `entry` of [`MAKER`]'s list, blocking the pointer as `blocks` says.
fn reaching(entry: u32, blocks: bool) -> Reaching {
    let actor = ProtocolRef::found(Seated { id: MAKER, seat: SeatNumber::new(1) });

    Reaching { actor, entry, blocks }
}

/// The entry `entry` of [`MAKER`]'s list, drawn at the window's origin, which blocks the pointer as `blocks` says.
pub(super) fn reach(entry: u32, blocks: bool) -> Reach {
    reaching(entry, blocks).at(Vec2::ZERO)
}

/// What reaches the entry `entry` of [`MAKER`]'s list, blocking the pointer as `blocks` says.
pub(super) fn reaches(entry: u32, blocks: bool) -> Picks {
    Picks::Reaches(reaching(entry, blocks))
}

/// A hit in the window's main frame's pass [`Main`], over `rect`, reaching what `picks` says.
pub(super) fn hit(rect: Rect2, picks: Picks) -> Listed {
    Listed::Hit { frame: Frame::Main(window()), pass: PassName::of::<Main>(), rect, picks }
}

/// A quad by [`MAKER`] over the whole window, tinted grey.
fn whole(gpu: &Gpu, kept: &mut Kept, program: ProgramId) -> PlacementId {
    placed(gpu, kept, (MAKER, program), (-1.0, 1.0), [0.5, 0.5, 0.5, 0.5])
}

/// A window 4 by 4 of the window's format, to draw into and copy from.
pub(super) fn window_frame(device: &wgpu::Device) -> wgpu::Texture {
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
pub(super) fn probed(gpu: &Gpu, kept: &mut Kept, frame: &wgpu::Texture, (x, y, pressed): (f32, f32, bool)) -> Stacks {
    let (device, queue) = (gpu.device(), gpu.queue());
    kept.fit(device, window(), Size { width: frame.width(), height: frame.height() });
    let heard = Probed { window: window(), at: 1, x, y, press: pressed };
    for windowed in &mut kept.windows {
        windowed.probing.heard(&heard);
    }
    let windows = [(window(), frame.create_view(&wgpu::TextureViewDescriptor::default()))];
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let (_, mut stacks, _) = kept.run_all(&mut encoder, (device, queue), &windows, None, &mut Tally::Off);
    queue.submit([encoder.finish()]);

    let Some(stacks) = stacks.pop() else {
        panic!("the window is probed");
    };

    stacks
}

/// What a GPU would have left at the probes: at the first, the calls `pointer`, the highest first, and at the second,
/// `press`.
pub(super) fn written(pointer: &[u32], press: &[u32]) -> Vec<u8> {
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

/// The stack the GPU writes at the pointer, at the middle of the window's second row's second pixel, as `kept` on
/// `gpu` draws what it lists.
fn read_back(gpu: &Gpu, kept: &mut Kept) -> Vec<Reach> {
    read_back_at(gpu, kept, (1.5, 1.5))
}

/// The stack the GPU writes at the pointer, at `x`, `y` of the window, as `kept` on `gpu` draws what it lists.
pub(super) fn read_back_at(gpu: &Gpu, kept: &mut Kept, (x, y): (f32, f32)) -> Vec<Reach> {
    let frame = window_frame(gpu.device());
    let stacks = probed(gpu, kept, &frame, (x, y, false));
    stacks.buffer.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the stacks map"));
    gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
    let Some((_, picked)) = mapped_bytes(&stacks.buffer, |bytes| stacks.picked(bytes)) else {
        panic!("the stacks are read");
    };

    picked.pointer
}

/// The stack the GPU writes under the pointer once `kept` on `gpu` draws `count` overlapping quads, each over the
/// whole window, the pointer reaching each but the last, listed in that order, each its own entry.
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

    read_back(gpu, &mut kept)
}

/// The stack the GPU writes under the pointer once `kept` on `gpu` draws two quads over the whole window in the pass
/// [`Main`], which tests depth, each writing its depth, a near one, entry 0, and a far one, entry 1, called in
/// `order`.
fn at_depths(gpu: &Gpu, order: [usize; 2]) -> Vec<Reach> {
    let mut kept = kept(gpu.device());
    let program = quads(gpu, &mut kept, DEEP, Depth::TestWrite);
    let listed = (MAKER, program);
    let near = placed(gpu, &mut kept, listed, (-1.0, 1.0), [1.0, 0.0, 0.0, 0.25]);
    let far = placed(gpu, &mut kept, listed, (-1.0, 1.0), [0.0, 0.0, 1.0, 0.75]);
    let entries = [(near, 0), (far, 1)];
    let calls = order.map(|at| drawn(entries[at].0, reaches(entries[at].1, false))).to_vec();
    assert_eq!(kept.listed(calls, MAKER), Ok(()));

    read_back(gpu, &mut kept)
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

// What the step writes on a GPU in a pass that tests depth: under a near quad and a far one, the near one, which is
// what is seen there, stands on top, whether it is called first or last. A machine with no adapter, as CI's, skips it
// and says so.
#[test]
fn on_a_gpu_the_nearest_stands_on_top_in_a_pass_that_tests_depth_whatever_the_order_of_the_calls() {
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to pick on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let gpu = Gpu::new(device, queue);
    let near_on_top = vec![reach(0, false), reach(1, false)];

    assert_eq!(at_depths(&gpu, [0, 1]), near_on_top, "near called first, on {adapter}");
    assert_eq!(at_depths(&gpu, [1, 0]), near_on_top, "near called last, on {adapter}");
}
