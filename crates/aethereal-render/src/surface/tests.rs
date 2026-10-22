//! The front's tests: it holds at most one frame, the last the chassis put, on a device that draws nothing.

use super::Front;

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

#[test]
fn the_front_holds_the_last_frame_put_once_and_none_once_cleared() {
    // Catches a frame put while the last was not taken left behind it, which the render root would take and draw
    // into once the chassis had dropped the surface's frame it came from, a frame taken twice, and a frame the
    // chassis took back still there for the render root.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (front, handing) = Front::new();

    handing.put(frame(&device, 1));
    handing.put(frame(&device, 2));
    assert_eq!(front.take().map(|taken| taken.width()), Some(2));
    assert_eq!(front.take().map(|taken| taken.width()), None);

    handing.put(frame(&device, 3));
    handing.clear();
    assert_eq!(front.take().map(|taken| taken.width()), None);
}
