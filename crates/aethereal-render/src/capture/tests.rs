use super::image::{Region, cut, png, sent_size, shrunk};
use super::{Crop, said};
use std::io::Cursor;

/// One pixel of RGBA, opaque unless said.
const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> [u8; 4] {
    [red, green, blue, alpha]
}

/// Whether `byte` is `expected`, give or take one for rounding.
fn near(byte: u8, expected: u8) -> bool {
    byte.abs_diff(expected) <= 1
}

#[test]
fn the_longer_side_is_shrunk_to_longest_keeping_the_shape_and_nothing_is_enlarged() {
    // Catches the shorter side bounded instead of the longer, a small frame enlarged, the shape lost to a scale applied
    // to one side, and a thin frame's shorter side rounded to nothing.
    assert_eq!(sent_size(2048, 1280, 1024), (1024, 640));
    assert_eq!(sent_size(1280, 2048, 1024), (640, 1024));
    assert_eq!(sent_size(800, 600, 1024), (800, 600));
    assert_eq!(sent_size(1024, 1024, 1024), (1024, 1024));
    assert_eq!(sent_size(4000, 3, 100), (100, 1));
}

#[test]
fn a_region_is_within_a_frame_only_when_it_is_neither_empty_nor_past_an_edge() {
    // Catches a region past the right or bottom edge taken, one whose edge overflows a u32 taken, and an empty one
    // taken, which would encode an image of no pixels.
    assert!(Region { x: 0, y: 0, width: 4, height: 3 }.within(4, 3));
    assert!(Region { x: 3, y: 2, width: 1, height: 1 }.within(4, 3));
    assert!(!Region { x: 3, y: 0, width: 2, height: 1 }.within(4, 3));
    assert!(!Region { x: 0, y: 2, width: 1, height: 2 }.within(4, 3));
    assert!(!Region { x: u32::MAX, y: 0, width: 2, height: 1 }.within(4, 3));
    assert!(!Region { x: 0, y: 0, width: 0, height: 1 }.within(4, 3));
}

#[test]
fn a_cut_takes_the_regions_pixels_row_by_row_without_their_alpha() {
    // Catches rows or columns taken from the wrong offset, a row's stride taken as the region's width rather than the
    // frame's, and the alpha kept, which would shift every pixel after the first.
    let frame: Vec<u8> = (0..3_u8).flat_map(|y| (0..3_u8).flat_map(move |x| rgba(x, y, 9, 128))).collect();

    let rgb = cut(&frame, 3, Region { x: 1, y: 1, width: 2, height: 2 });

    assert_eq!(rgb, vec![1, 1, 9, 2, 1, 9, 1, 2, 9, 2, 2, 9]);
}

#[test]
fn a_shrunk_pixel_is_the_mean_of_the_light_it_covers() {
    // Catches the mean taken of the encoded bytes, which darkens: black beside white is 128 so, and 188 in light; a
    // thin bright line lost or dimmed below a quarter of its light; a pixel's share of a wider one weighted wrongly;
    // and an image of its own size changed.
    let half = shrunk(&[0, 0, 0, 255, 255, 255], 2, 1, 1, 1);
    assert!(half.iter().all(|byte| near(*byte, 188)), "{half:?}");

    let line = [0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0];
    let quarter = shrunk(&line, 4, 1, 1, 1);
    assert!(quarter.iter().all(|byte| near(*byte, 137)), "{quarter:?}");

    // Three across to two: the first covers all of the white and half of the next.
    let row = [255, 255, 255, 0, 0, 0, 0, 0, 0];
    let shared = shrunk(&row, 3, 1, 2, 1);
    assert!(shared.iter().take(3).all(|byte| near(*byte, 213)), "{shared:?}");
    assert!(shared.iter().skip(3).all(|byte| *byte == 0), "{shared:?}");

    let kept = [12, 34, 56, 78, 90, 12];
    assert_eq!(shrunk(&kept, 2, 1, 2, 1), kept.to_vec());
}

#[test]
fn a_png_holds_the_pixels_as_rgb_and_says_they_are_srgb() {
    // Catches the pixels written with an alpha channel or another depth, rows written out of order, and the sRGB chunk
    // left off, which leaves a viewer to guess how to show them.
    let rgb: Vec<u8> = (0..2 * 3 * 3).map(|at| u8::try_from(at * 9).unwrap_or(u8::MAX)).collect();
    let Ok(written) = png(&rgb, 2, 3, Vec::new()) else {
        panic!("the PNG is written");
    };

    let Ok(mut reader) = png::Decoder::new(Cursor::new(written)).read_info() else {
        panic!("the PNG reads");
    };
    let mut read = vec![0; rgb.len()];
    let Ok(frame) = reader.next_frame(&mut read) else {
        panic!("the PNG's image reads");
    };
    assert_eq!((frame.width, frame.height), (2, 3));
    assert_eq!((frame.color_type, frame.bit_depth), (png::ColorType::Rgb, png::BitDepth::Eight));
    assert_eq!(read, rgb);
    assert!(reader.info().srgb.is_some(), "the PNG is marked as sRGB");
}

#[test]
fn the_line_beside_an_image_says_the_frames_size_the_crop_and_the_images_size() {
    // Catches the frame's size written as the crop's or the image's, which would map a next crop to the wrong pixels.
    assert_eq!(said(2048, 1280, Crop::Whole, 1024, 640), "frame 2048x1280, crop whole, image 1024x640");
    let corner = Crop::Region { x: 10, y: 20, width: 300, height: 200 };
    assert_eq!(said(2048, 1280, corner, 300, 200), "frame 2048x1280, crop 300x200 at 10,20, image 300x200");
}
