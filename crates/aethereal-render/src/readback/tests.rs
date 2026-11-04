//! Reading back's layout: rows padded as WebGPU copies them, and cut back to the pixels as they are answered; and a
//! region of a frame's end scaled to its target.

use super::*;

#[test]
fn a_copied_row_is_padded_to_whole_rows_of_256_bytes() {
    // Catches a row left unpadded, which WebGPU refuses to copy into, or padded one row too far, which reads each
    // row from the wrong place.
    assert_eq!(Copied::new(64, 2, 4).row, 256);
    assert_eq!(Copied::new(65, 2, 4).row, 512);
    assert_eq!(Copied::new(3, 2, 1).row, 256);
    assert_eq!(Copied::new(65, 3, 4).bytes(), 1536);
}

#[test]
fn unpadding_keeps_each_rows_pixels_in_order_and_drops_the_padding() {
    // Catches a row read from the wrong offset, padding left in, or the rows' order lost: two rows of two pixels,
    // each row padded to 256 bytes with bytes that are not pixels.
    let copied = Copied::new(2, 2, 4);
    let mut mapped = vec![0xee; 512];
    mapped[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    mapped[256..264].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);

    assert_eq!(copied.unpad(&mapped, Order::Kept), (1..=16).collect::<Vec<u8>>());
}

#[test]
fn a_canvas_of_blue_first_is_answered_red_first() {
    // Catches channels answered as the canvas holds them: a BGRA canvas, as browsers often prefer, would read back
    // red as blue.
    let copied = Copied::new(2, 1, 4);
    let mut mapped = vec![0; 256];
    mapped[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

    assert_eq!(copied.unpad(&mapped, Order::Bgra), [3, 2, 1, 4, 7, 6, 5, 8]);
}

#[test]
fn a_frame_is_answered_in_a_format_only_from_four_bytes_a_pixel() {
    // Catches a canvas's order or encoding answered wrong, or a format of other pixels read as four bytes.
    let rgba = frame_format(wgpu::TextureFormat::Rgba8Unorm);
    let bgra = frame_format(wgpu::TextureFormat::Bgra8UnormSrgb);
    let wide = frame_format(wgpu::TextureFormat::Rgba16Float);

    assert_eq!(rgba, Some((TextureFormat::Rgba8, Order::Kept)));
    assert_eq!(bgra, Some((TextureFormat::Rgba8Srgb, Order::Bgra)));
    assert_eq!(wide, None);
}

#[test]
fn a_region_of_the_end_is_read_at_the_target_pixels_that_cover_it() {
    // Catches a scaled region that drops the last target pixel a region's far edge reaches into, one rounded toward
    // its centre rather than out, one reaching past the end read past the target rather than cut to it, and a region
    // wholly past the end, or empty, read rather than refused.
    let end = Size { width: 101, height: 50 };
    let pixel = |x, y| Region { x, y, width: 1, height: 1 };
    assert_eq!(pixel(100, 49).scaled(end, Scale::Full), Some(pixel(100, 49)));
    assert_eq!(pixel(100, 49).scaled(end, Scale::Half), Some(pixel(50, 24)));
    let across = Region { x: 3, y: 0, width: 6, height: 2 };
    assert_eq!(across.scaled(end, Scale::Quarter), Some(Region { x: 0, y: 0, width: 3, height: 1 }));

    let edge = Region { x: 99, y: 48, width: 5, height: 5 };
    assert_eq!(edge.scaled(end, Scale::Full), Some(Region { x: 99, y: 48, width: 2, height: 2 }));
    assert_eq!(pixel(101, 0).scaled(end, Scale::Full), None);
    assert_eq!(Region { x: 0, y: 0, width: 0, height: 1 }.scaled(end, Scale::Full), None);
    assert_eq!(Region { x: u32::MAX, y: 0, width: 2, height: 1 }.scaled(end, Scale::Full), None);
}

#[test]
fn a_region_is_cut_from_the_rows_it_was_copied_in() {
    // Catches a region cut from the wrong row, as its padding misplaces it, or from the wrong pixel of each row.
    let copied = Copied::new(4, 3, 4);
    let mapped: Vec<u8> = (0..3)
        .flat_map(|row: u8| {
            let mut padded: Vec<u8> = (0..16).map(|byte| row * 16 + byte).collect();
            padded.resize(copied.row as usize, 0xff);
            padded
        })
        .collect();
    let cut = copied.cut(&mapped, Region { x: 1, y: 1, width: 2, height: 2 });
    assert_eq!(cut, [20, 21, 22, 23, 24, 25, 26, 27, 36, 37, 38, 39, 40, 41, 42, 43]);
}
