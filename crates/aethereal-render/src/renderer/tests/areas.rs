//! A call places its child (ADR-0055): on the device that draws nothing, a hit moved to its list's origin and cut to
//! its clip as the pointer is tested against it; and, where the machine has a GPU adapter, the pixels a child draws
//! once its caller moves and cuts it, and where the pointer reaches it.

// The GPU tests say which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::lists::{CHILD, FRAME_SIDE, adapter_device, called, drawn, frame_pixels, placed, tinted};
use super::picked::{hit, probed, reach, reaches, read_back_at, window_frame, written};
use super::*;
use crate::draw_list::Clip;
use aethereal_window::Reach;

/// The rect from `min` to `max`, in the window's pixels.
const fn rect(min: [f32; 2], max: [f32; 2]) -> Rect2 {
    Rect2 { min: Vec2::new(min[0], min[1]), max: Vec2::new(max[0], max[1]) }
}

/// The renderer's kept state on `gpu` with one quad by [`CHILD`], opaque red, from `left` to `right` across and top to
/// bottom of the child's own clip space as its program places it, reached by the pointer as `picks` says, and
/// [`MAKER`]'s list calling the child in `area`.
fn child_in(gpu: &Gpu, area: Area, (left, right): (f32, f32), picks: Picks) -> Kept {
    let mut kept = kept(gpu.device());
    let program = tinted(gpu, &mut kept);
    let quad = placed(gpu, &mut kept, (CHILD, program), (left, right), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(kept.listed(vec![drawn(quad, picks)], CHILD), Ok(()));
    assert_eq!(kept.listed(vec![called(CHILD, area)], MAKER), Ok(()));

    kept
}

/// Which pixels of the window's frame are red once `kept` on `gpu` draws it, row by row.
fn red(gpu: &Gpu, kept: &mut Kept) -> Vec<Vec<bool>> {
    let pixels = frame_pixels(gpu, kept);

    pixels
        .chunks_exact(FRAME_SIDE as usize)
        .map(|row| row.iter().map(|[red, green, blue, _]| *red > 128 && *green < 64 && *blue < 64).collect())
        .collect()
}

/// A GPU to draw on, beside its adapter's name, or none when the machine has no adapter, as CI's, said so.
fn gpu() -> Option<(Gpu, String)> {
    match adapter_device() {
        Ok((device, queue, adapter)) => {
            eprintln!("ran adapter={adapter:?}");
            Some((Gpu::new(device, queue), adapter))
        }
        Err(why) => {
            eprintln!("skipped: no GPU adapter to place children on: {why}");
            None
        }
    }
}

// A child's drawing lands at the origin its caller gives, in the window's pixels, down as well as across. Catches a
// call's origin dropped, so every child draws at the surface's corner; one moved in clip space by a share of the
// window other than its pixels; and one moved up rather than down, as clip space runs up. A machine with no adapter,
// as CI's, skips it and says so.
#[test]
fn on_a_gpu_a_child_lands_at_the_origin_its_caller_gives_it() {
    let Some((gpu, adapter)) = gpu() else {
        return;
    };
    let placed = Area { origin: Vec2::new(1.0, 1.0), clip: Clip::Whole };
    let mut kept = child_in(&gpu, placed, (-1.0, 0.0), Picks::Ignores);

    let lit = red(&gpu, &mut kept);

    let row = vec![false, true, true, false];
    assert_eq!(lit, vec![vec![false; 4], row.clone(), row.clone(), row], "on {adapter}");
}

// A child moved left by a whole window shows what it draws past its own surface-sized rect, as a scroll view's
// content does. Catches a call placed with the GPU's viewport, which clips a child's drawing to its own clip space
// before it is moved, so a negative origin leaves the window blank. A machine with no adapter, as CI's, skips it and
// says so.
#[test]
fn on_a_gpu_a_child_at_a_negative_origin_shows_what_lies_past_its_own_surface_sized_rect() {
    let Some((gpu, adapter)) = gpu() else {
        return;
    };
    // The quad lies from the child's fourth pixel to its eighth: wholly right of a window four pixels wide.
    let scrolled = Area { origin: Vec2::new(-4.0, 0.0), clip: Clip::Whole };
    let mut kept = child_in(&gpu, scrolled, (1.0, 3.0), Picks::Ignores);

    let lit = red(&gpu, &mut kept);

    assert_eq!(lit, vec![vec![true; 4]; 4], "on {adapter}");
}

// What a child draws outside its clip, given in its caller's coordinates, is cut. Catches a clip that cuts nothing;
// and one taken in the child's coordinates, moved by its origin, which here would let the last column through. A
// machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_what_a_child_draws_outside_its_clip_is_cut() {
    let Some((gpu, adapter)) = gpu() else {
        return;
    };
    let cut = Area { origin: Vec2::new(1.0, 0.0), clip: Clip::To(rect([0.0, 1.0], [3.0, 3.0])) };
    let mut kept = child_in(&gpu, cut, (-1.0, 1.0), Picks::Ignores);

    let lit = red(&gpu, &mut kept);

    let row = vec![false, true, true, false];
    assert_eq!(lit, vec![vec![false; 4], row.clone(), row, vec![false; 4]], "on {adapter}");
}

// The pointer reaches a child where it is drawn, moved, told with the origin it was drawn at, and not where its clip
// cuts it. Catches picking left at where the child's program placed it, so a click on a moved child goes to what is
// beneath it, and a click beside it to the child; and a draw's fragments picked past its clip, so a scroll view's
// hidden rows take clicks meant for what is drawn there. A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_a_child_is_picked_where_it_is_drawn_and_not_where_its_clip_cuts_it() {
    let Some((gpu, adapter)) = gpu() else {
        return;
    };
    let placed = Area { origin: Vec2::new(2.0, 0.0), clip: Clip::To(rect([0.0, 0.0], [3.0, 4.0])) };
    let mut kept = child_in(&gpu, placed, (-1.0, 1.0), reaches(0, true));

    let unmoved = read_back_at(&gpu, &mut kept, (1.5, 1.5));
    let drawn = read_back_at(&gpu, &mut kept, (2.5, 1.5));
    let cut = read_back_at(&gpu, &mut kept, (3.5, 1.5));

    assert_eq!(unmoved, vec![], "where the child's program placed it, on {adapter}");
    let at_origin = Reach { origin: Vec2::new(2.0, 0.0), ..reach(0, true) };
    assert_eq!(drawn, vec![at_origin], "where it is drawn, on {adapter}");
    assert_eq!(cut, vec![], "where its clip cuts it, on {adapter}");
}

#[test]
fn a_hit_is_moved_by_its_list_s_origin_and_cut_to_its_clip() {
    // Catches a hit tested at its rect as its list gave it, in the child's coordinates, so a handle in a panel moved
    // across the window is reached where the panel was; and one reached past its list's clip, so a hidden row of a
    // scroll view blocks the pointer from what is drawn over it (ADR-0055).
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let frame = window_frame(&device);
    let handles =
        vec![hit(rect([0.0, 0.0], [1.0, 4.0]), reaches(0, true)), hit(rect([1.0, 0.0], [2.0, 4.0]), reaches(1, true))];
    assert_eq!(kept.listed(handles, CHILD), Ok(()));
    let placed = Area { origin: Vec2::new(2.0, 0.0), clip: Clip::To(rect([0.0, 0.0], [3.0, 4.0])) };
    assert_eq!(kept.listed(vec![called(CHILD, placed)], MAKER), Ok(()));
    let mut at = |x| probed(&gpu, &mut kept, &frame, (x, 1.5, false)).picked(&written(&[], &[])).1.pointer;

    let (unmoved, moved, cut) = (at(0.5), at(2.5), at(3.5));

    assert_eq!(unmoved, vec![], "where the child's list gave it");
    assert_eq!(moved, vec![Reach { origin: Vec2::new(2.0, 0.0), ..reach(0, true) }]);
    assert_eq!(cut, vec![], "past the child's clip");
}
