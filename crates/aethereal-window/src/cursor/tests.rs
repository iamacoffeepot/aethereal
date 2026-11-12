//! The cursor's tests: which images can be shown, and that a host makes each distinct image into a cursor once.

use super::*;
use aethereal_substrate::Engine;
use std::num::NonZeroUsize;

/// An engine with room for a few small blobs.
fn engine() -> Engine {
    Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>().ready()
}

/// A `side` by `side` image of `pixels`, its hotspot at its top left.
const fn square(pixels: Bytes, side: u16) -> CursorImage {
    CursorImage { pixels, width: side, height: side, hotspot_x: 0, hotspot_y: 0 }
}

#[test]
fn an_image_whose_pixels_its_size_does_not_account_for_or_whose_hotspot_is_outside_it_is_unfit() {
    // Catches a host handed pixels that are not four bytes for each pixel, which winit refuses and a page draws
    // skewed; a hotspot past the image, which points from nowhere; and a side past what a browser shows.
    let mut engine = engine();
    let Ok(pixels) = engine.put_blob(vec![0; 2 * 2 * 4]) else {
        panic!("the engine has room for the image");
    };
    let image = square(pixels, 2);

    assert_eq!(image.check(2 * 2 * 4), Ok(()));
    assert_eq!(image.check(2 * 2 * 3), Err(CursorUnfit::Pixels));
    assert_eq!(CursorImage { hotspot_x: 2, ..image.clone() }.check(16), Err(CursorUnfit::Hotspot));
    assert_eq!(CursorImage { height: 0, ..image.clone() }.check(0), Err(CursorUnfit::Size));
    let past = MOST_CURSOR_SIDE + 1;
    let long = CursorImage { width: past, ..image };
    assert_eq!(long.check(usize::from(past) * 2 * 4), Err(CursorUnfit::Size));
}

#[test]
fn one_image_is_made_into_one_cursor_however_often_it_is_asked_for_and_whatever_blob_holds_it() {
    // Catches a host that makes a new cursor each time an image is shown, as each hover over a draggable item and
    // each drag would, so its cursors grow without end; one kept by blob, so an image put again each frame is made
    // again each frame; and one kept by its pixels alone, so the same image with another hotspot points from the first.
    let mut engine = engine();
    let bytes = vec![255; 4 * 4 * 4];
    let (Ok(first), Ok(again)) = (engine.put_blob(bytes.clone()), engine.put_blob(bytes)) else {
        panic!("the engine has room for both images");
    };
    let mut made = MadeCursors::default();
    let mut makes = Vec::new();

    for image in [square(first.clone(), 4), square(first.clone(), 4), square(again, 4)] {
        let cursor = made.of(&mut engine, &image, |image, pixels| {
            makes.push(pixels.len());
            Ok::<_, ()>(image.hotspot_x)
        });
        assert_eq!(cursor, Ok(&0));
    }
    assert_eq!(makes, vec![4 * 4 * 4], "made once, of its pixels");

    let elsewhere = CursorImage { hotspot_x: 3, ..square(first, 4) };
    let cursor = made.of(&mut engine, &elsewhere, |image, _| Ok::<_, ()>(image.hotspot_x));
    assert_eq!(cursor, Ok(&3), "another hotspot is another cursor");
}
