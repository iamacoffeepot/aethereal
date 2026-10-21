//! Reading back's layout: rows padded as WebGPU copies them, and cut back to the pixels as they are answered.

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
