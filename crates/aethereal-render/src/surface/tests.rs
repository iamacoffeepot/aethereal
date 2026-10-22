//! The front's tests: each window's slot holds at most one frame, the last the chassis put for that window, on a device
//! that draws nothing; a slot added or taken out between takes is handed from the next; and an ask to present one
//! window's frame is that window's alone.

use super::{Front, Present};
use aethereal_data::{Budget, from_bytes};
use aethereal_window::WindowId;

/// The window opened `nth`, as a host's window capability numbers it.
fn window(nth: u64) -> WindowId {
    let Ok(window) = from_bytes(&nth.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// A texture of one pixel, `width` across, to tell the frames apart by.
fn frame(device: &wgpu::Device, width: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// What the front hands this cycle, each window's frame by its width.
fn taken(front: &mut Front) -> Vec<(WindowId, u32)> {
    front.take().into_iter().map(|(window, frame)| (window, frame.width())).collect()
}

#[test]
fn each_window_slot_holds_the_last_frame_put_for_it_once_and_none_once_cleared() {
    // Catches a frame put while the last was not taken left behind it, which the render root would take and draw
    // into once the chassis had dropped the surface's frame it came from; a frame taken twice; a frame the chassis
    // took back still there for the render root; and a frame handed under another window's id, or put in place of
    // another window's, which would draw one window's main frame into the other's surface or leave it undrawn.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (mut front, fronting) = Front::new();
    let (first, second) = (fronting.hand(window(0)), fronting.hand(window(1)));

    first.put(frame(&device, 1));
    first.put(frame(&device, 2));
    second.put(frame(&device, 3));
    assert_eq!(taken(&mut front), [(window(0), 2), (window(1), 3)]);
    assert_eq!(taken(&mut front), []);

    first.put(frame(&device, 4));
    second.put(frame(&device, 5));
    first.clear();
    assert_eq!(taken(&mut front), [(window(1), 5)]);
}

#[test]
fn a_slot_added_after_the_front_is_handed_is_taken_from_and_one_taken_out_hands_nothing() {
    // Catches a window opened while the program runs never handing the render root its frames, its slot not reaching
    // the front it holds; a window closed still handing the frame put before it closed, which the render root would
    // draw into once the surface it came from was dropped; and a slot added again for a window, as its surface is made
    // anew, handing the old slot's frame beside the new one's.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (mut front, fronting) = Front::new();
    let first = fronting.hand(window(0));
    first.put(frame(&device, 1));
    assert_eq!(taken(&mut front), [(window(0), 1)]);

    let second = fronting.hand(window(1));
    first.put(frame(&device, 2));
    second.put(frame(&device, 3));
    assert_eq!(taken(&mut front), [(window(0), 2), (window(1), 3)]);

    first.put(frame(&device, 4));
    second.put(frame(&device, 5));
    fronting.remove(window(0));
    let again = fronting.hand(window(1));
    again.put(frame(&device, 6));
    assert_eq!(taken(&mut front), [(window(1), 6)]);
}

#[test]
fn an_ask_to_present_one_window_presents_that_window_alone() {
    // Catches a surface that presents whenever anything was asked, as one window's surface did, which would present
    // a window's frame the render root never drew into, as when its main frame was not drawn that cycle.
    let asked = [Present::of(window(1))];

    assert!(Present::asked_for(&asked, window(1)));
    assert!(!Present::asked_for(&asked, window(0)));
    assert!(!Present::asked_for(&[], window(1)));
}
