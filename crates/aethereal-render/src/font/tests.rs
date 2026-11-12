//! Reading a real font: Noto Sans Regular, whose character map, composite glyphs, and `GPOS` pair kerning of both
//! formats the reader must get right, and whose bytes cut short or garbled it must refuse without a panic.

use super::outline::{Contour, Segment};
use super::*;
use aethereal_math::Vec2;

/// Noto Sans Regular (SIL OFL), copied unchanged from Debian's `fonts-noto-core`.
pub const NOTO: &[u8] = include_bytes!("../../fixtures/fonts/NotoSans-Regular.ttf");

/// The fixture font, read.
pub fn noto() -> Font {
    Font::read(NOTO).expect("Noto Sans reads")
}

/// The glyph of `character`, which the fixture has.
fn glyph(font: &Font, character: char) -> GlyphId {
    font.metrics().glyph(character).expect("the fixture draws the character")
}

/// `outline`'s contours with every point mapped by `map`.
fn mapped(outline: &Outline, map: impl Fn(Vec2) -> Vec2 + Copy) -> Vec<Contour> {
    outline
        .contours
        .iter()
        .map(|contour| Contour { segments: contour.segments.iter().map(|segment| segment.map(map)).collect() })
        .collect()
}

#[test]
fn the_character_map_finds_each_character_and_misses_what_the_font_lacks() {
    // Catches a format 4 segment read off by one, its delta added wrongly, or its glyph array indexed from the wrong
    // place: a letter would draw another letter's glyph. The glyph numbers are the font's, read apart with Python.
    let font = noto();

    let found = ['A', 'V', 'o', '@', '\u{e9}', '\u{1d11}'].map(|character| font.metrics().glyph(character));
    let lacked = ['\u{4e2d}', '\u{1f600}'].map(|character| font.metrics().glyph(character));

    let expected = [36, 57, 82, 35, 171, 1263].map(|number| Some(GlyphId(number)));
    assert_eq!(found, expected);
    assert_eq!(lacked, [None, None]);
}

#[test]
fn a_composite_glyph_is_its_components_transformed_and_moved() {
    // Catches a component's offset dropped, a 2×2 transform read with its middle terms swapped (which turns a quarter
    // turn the wrong way), or a negative scale lost: accented letters and mirrored glyphs would draw wrongly. Each
    // composite's components, transforms, and offsets were read apart with Python.
    let font = noto();
    let outline = |glyph: u16| font.outline(GlyphId(glyph)).expect("the fixture has the glyph");

    // é: e, then the acute moved 185 units right.
    let acute = [mapped(outline(72), |at| at), mapped(outline(118), |at| at + Vec2::new(185.0, 0.0))].concat();
    // ᴑ: o turned a quarter, x′ = y and y′ = −x, then moved by (28, 571).
    let turned = mapped(outline(82), |at| Vec2::new(at.y + 28.0, -at.x + 571.0));
    // ¿: ? scaled by −1, then moved by (432, 532).
    let inverted = mapped(outline(34), |at| Vec2::new(432.0 - at.x, 532.0 - at.y));

    assert_eq!(outline(171).contours, acute);
    assert_eq!(outline(1263).contours, turned);
    assert_eq!(outline(129).contours, inverted);
}

#[test]
fn pairs_the_font_kerns_are_kerned_and_others_are_not() {
    // Catches `GPOS` pair adjustment read wrong: a class table indexed by the wrong class, a value record's advance read
    // from the wrong field, or the `kern` feature's lookups missed. A and V, and T and o, are drawn together in every
    // Latin font; H and H, two stems, are not.
    let font = noto();
    let pair = |left: char, right: char| font.metrics().kerning(glyph(&font, left), glyph(&font, right));

    assert!(pair('A', 'V') < 0, "A V kerns by {}", pair('A', 'V'));
    assert!(pair('T', 'o') < 0, "T o kerns by {}", pair('T', 'o'));
    assert_eq!(pair('H', 'H'), 0);
}

#[test]
fn bytes_cut_short_are_refused_never_a_panic() {
    // Catches a read past a table's end that is not checked: a font cut short by a failed download would panic the
    // render root. Every cut before the end of `GPOS`, the last table read, must be refused, and none may panic.
    let gpos_end = 437_140 + 67_006;
    let cuts = (0..gpos_end).step_by(4099);
    let refused = cuts.clone().filter(|&length| Font::read(&NOTO[..length]).is_err()).count();

    assert_eq!(refused, cuts.count());
}

#[test]
fn garbled_bytes_are_refused_or_read_never_a_panic() {
    // Catches a count, offset, or length taken on trust: a garbled table could index past its end, overflow, nest
    // without end, or allocate without bound. Each garbling of the tables' headers and the start of `glyf` is read, and
    // whatever reads is shaped, and must not panic.
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..48 {
        let mut garbled = NOTO.to_vec();
        for _ in 0..8 {
            let at = usize::try_from(next() % 40_000).expect("fits");
            garbled[at] = garbled[at].wrapping_add(u8::try_from(next() % 255 + 1).expect("fits"));
        }
        if let Ok(font) = Font::read(&garbled) {
            let shaped = font.metrics().shape("Sphinx of black quartz, judge my vow.\nAV To \u{e9}");
            assert_eq!(shaped.lines.len(), 2);
        }
    }
}

#[test]
fn a_font_that_is_not_truetype_names_what_it_is() {
    // Catches the container's tag left unread: a WOFF2 or CFF font would fail later as a missing table, and the
    // refusal would not say what to do about it.
    let tagged = |tag: &[u8; 4]| [tag.as_slice(), &NOTO[4..]].concat();

    let refused = [*b"OTTO", *b"wOF2", *b"wOFF", *b"ttcf", *b"%PDF"].map(|tag| Font::read(&tagged(&tag)).err());

    let expected = [
        FontRefused::Cff,
        FontRefused::Container(Container::Woff2),
        FontRefused::Container(Container::Woff),
        FontRefused::Container(Container::Collection),
        FontRefused::Container(Container::Unknown),
    ]
    .map(Some);
    assert_eq!(refused, expected);
}

#[test]
fn every_glyph_of_the_font_is_closed_contours() {
    // Catches a contour walked from the wrong point, or not closed back to its start: the distance field's sign would
    // leak out of the glyph through the gap.
    let font = noto();
    let contours = (0..font.glyph_count())
        .filter_map(|glyph| font.outline(GlyphId(u16::try_from(glyph).expect("fits"))))
        .flat_map(|outline| &outline.contours);

    for contour in contours {
        let ends = contour.segments.iter().map(|segment| segment.to());
        let starts = contour.segments.iter().cycle().skip(1).map(|segment| segment.from());
        let gaps = ends.zip(starts).filter(|(end, start)| end != start).count();
        assert_eq!(gaps, 0, "{:?}", contour.segments.first().map(|segment: &Segment| segment.from()));
    }
}
