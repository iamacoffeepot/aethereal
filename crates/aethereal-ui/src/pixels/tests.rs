//! Decoding: each PNG colour type widened to RGBA, a JPEG made opaque RGBA, an image past the bounds refused from its
//! header, and broken bytes refused without a panic.

use super::{MOST_SIDE, Pixels, Undecoded};

/// A JPEG 4 by 2 pixels of one red, as an image tool wrote it.
const RED_JPEG: &[u8] = include_bytes!("red.jpg");

/// A PNG `width` by `height` of `colour`, holding `data`, eight bits a channel.
fn png(width: u32, height: u32, colour: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(colour);
    encoder.set_depth(png::BitDepth::Eight);
    let Ok(mut writer) = encoder.write_header() else {
        panic!("the header is written");
    };
    let Ok(()) = writer.write_image_data(data) else {
        panic!("the pixels are written");
    };
    drop(writer);

    bytes
}

#[test]
fn each_png_colour_type_is_widened_to_rgba_and_one_without_alpha_is_opaque() {
    // Catches gray left one channel wide, which shifts every pixel after the first, and alpha left at zero where the
    // image has none, which draws it invisible.
    let cases = [
        (png::ColorType::Grayscale, vec![10, 20], vec![10, 10, 10, 255, 20, 20, 20, 255]),
        (png::ColorType::GrayscaleAlpha, vec![10, 1, 20, 2], vec![10, 10, 10, 1, 20, 20, 20, 2]),
        (png::ColorType::Rgb, vec![1, 2, 3, 4, 5, 6], vec![1, 2, 3, 255, 4, 5, 6, 255]),
        (png::ColorType::Rgba, vec![1, 2, 3, 4, 5, 6, 7, 8], vec![1, 2, 3, 4, 5, 6, 7, 8]),
    ];

    for (colour, data, rgba) in cases {
        let decoded = Pixels::decode(&png(2, 1, colour, &data));

        let Ok(pixels) = decoded else {
            panic!("a {colour:?} PNG decodes: {decoded:?}");
        };
        assert_eq!((pixels.size(), pixels.rgba()), ((2, 1), rgba.as_slice()), "{colour:?}");
    }
}

#[test]
fn a_jpeg_decodes_to_opaque_rgba_of_its_size() {
    // Catches a JPEG decoded as RGB, three bytes a pixel, which the texture refuses, or sized by something other than
    // its header.
    let decoded = Pixels::decode(RED_JPEG);

    let Ok(pixels) = decoded else {
        panic!("the JPEG decodes: {decoded:?}");
    };
    assert_eq!(pixels.size(), (4, 2));
    assert!(pixels.rgba().chunks_exact(4).all(|pixel| pixel[3] == 255 && pixel[0] > 150), "{:?}", pixels.rgba());
}

#[test]
fn an_image_wider_than_the_most_side_is_refused() {
    // Catches an untrusted image's size taken from its header unchecked, which makes a texture no GPU holds.
    let wide = MOST_SIDE + 1;
    let data = vec![0; usize::try_from(wide).unwrap_or(0)];

    let decoded = Pixels::decode(&png(wide, 1, png::ColorType::Grayscale, &data));

    assert_eq!(decoded.map(|pixels| pixels.size()), Err(Undecoded::Size { width: u64::from(wide), height: 1 }));
}

#[test]
fn broken_and_cut_images_are_refused_without_a_panic() {
    // Catches a panic on an untrusted image anywhere in decoding: each image cut at every length, and with bytes
    // changed through it.
    let whole_png = png(3, 2, png::ColorType::Rgb, &[7; 18]);
    for whole in [whole_png.as_slice(), RED_JPEG] {
        for cut in 0..whole.len() {
            let _ = Pixels::decode(&whole[..cut]);
        }
        for at in 0..whole.len() {
            let mut broken = whole.to_vec();
            broken[at] ^= 0xa5;
            let _ = Pixels::decode(&broken);
        }
    }
    assert_eq!(Pixels::decode(b"not an image"), Err(Undecoded::Format));
}
