use super::{SLATE, Swatch};

/// A channel on the curve and one on the straight line near black each come out in linear light, so a colour sent
/// as its sRGB fraction, too pale on an sRGB surface, is caught.
#[test]
fn channels_are_turned_into_linear_light() {
    let grey = Swatch::hex(0x80_80_80).opaque();
    let slate = SLATE.opaque();

    // Tripwire: 0x80 is 0.2158605 in linear light, and 0x08, on the straight line, 0.0024282.
    assert!((grey.red - 0.215_860_5).abs() < 1e-6);
    assert!((Swatch::hex(0x08_00_00).opaque().red - 0.002_428_2).abs() < 1e-6);
    assert!(slate.red < slate.green && slate.green < slate.blue);
}
