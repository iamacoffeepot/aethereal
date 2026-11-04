//! `read_target`'s own arithmetic: the region around a pixel, and the values a target's bytes hold.

use super::*;

#[test]
fn a_half_is_read_as_the_float_it_holds() {
    // Catches a half's exponent rebiased wrongly, which scales every float target's value, a subnormal's leading bit
    // kept as fraction, and an infinity or a sign lost.
    let cases = [
        (0x3c00, 1.0),
        (0xc000, -2.0),
        (0x3800, 0.5),
        (0x7bff, 65504.0),
        (0x0001, 2.0_f32.powi(-24)),
        (0x0200, 2.0_f32.powi(-15)),
        (0x8000, -0.0),
        (0x7c00, f32::INFINITY),
    ];
    for (bits, float) in cases {
        assert_eq!(half_to_float(bits).to_bits(), f32::to_bits(float), "{bits:#06x}");
    }
    assert!(half_to_float(0x7e00).is_nan());
}

#[test]
fn the_region_around_a_pixel_is_cut_at_the_top_and_left_and_centred_on_it() {
    // Catches a square around a pixel near the top left that underflows or shifts off the pixel asked, and one of
    // radius 0 that reads more than the pixel.
    let at = |x, y| Pixel { x, y };
    assert_eq!(around(at(10, 20), 0), Region { x: 10, y: 20, width: 1, height: 1 });
    assert_eq!(around(at(10, 20), 2), Region { x: 8, y: 18, width: 5, height: 5 });
    assert_eq!(around(at(1, 0), 3), Region { x: 0, y: 0, width: 5, height: 4 });
}

#[test]
fn a_target_s_bytes_are_read_as_its_kind_lays_them_out() {
    // Catches depth or an id read big-endian, and a float pixel's channels read out of order.
    let depth = [0.25_f32.to_le_bytes(), 1.0_f32.to_le_bytes()].concat();
    assert_eq!(values(TargetKind::Depth, &depth), Values::Depth(vec![0.25, 1.0]));
    let ids = [7_u32.to_le_bytes(), 0x0102_0304_u32.to_le_bytes()].concat();
    assert_eq!(values(TargetKind::Uint, &ids), Values::Uint(vec![7, 0x0102_0304]));
    let float = [0x3c00_u16, 0xc000, 0x3800, 0].iter().flat_map(|half| half.to_le_bytes()).collect::<Vec<u8>>();
    assert_eq!(values(TargetKind::Float, &float), Values::Float(vec![[1.0, -2.0, 0.5, 0.0]]));
}
