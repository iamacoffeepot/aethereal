//! Metrics as bytes: real fonts' metrics read back as exactly what was written, and bytes cut short, garbled, or out
//! of order refused, never a panic.

use super::*;
use crate::font::Font;
use crate::font::tests::{NOTO, noto};
use crate::text::faces::{MONO, SANS};

/// Metrics of 3 glyphs, mapping the characters of each of `groups` (first, last, first glyph) and kerning each of
/// `pairs` (left, right, adjustment) in one listed subtable, as `encode` writes them.
fn written(groups: &[(u32, u32, u32)], pairs: &[(u16, u16, i16)]) -> Vec<u8> {
    let mut writer = Writer::default();
    writer.u16(1000);
    for spacing in [800, -200, 0] {
        writer.i16(spacing);
    }
    writer.count(3);
    for advance in [500, 600, 700] {
        writer.u16(advance);
    }
    writer.count(groups.len());
    for &(start, end, glyph) in groups {
        writer.u32(start);
        writer.u32(end);
        writer.u32(glyph);
    }
    writer.count(1);
    writer.count(1);
    writer.u8(0);
    writer.count(pairs.len());
    for &(left, right, adjustment) in pairs {
        writer.u16(left);
        writer.u16(right);
        writer.i16(adjustment);
    }

    writer.into_bytes()
}

#[test]
fn a_font_s_metrics_read_back_as_exactly_the_metrics_it_was_read_as() {
    // Catches a part of the metrics left out of the bytes, written in another order than it is read, or narrowed: a
    // class list read as the other side's, a lookup's subtables merged, a line gap dropped. An actor would measure a
    // text other than the render root lays it out. Noto kerns by both forms of `GPOS` pairs; the renderer's own two are
    // what most text is measured in.
    for bytes in [NOTO, SANS, MONO] {
        let Ok(font) = Font::read(bytes) else {
            panic!("the font reads");
        };

        let decoded = Metrics::decode(&font.metrics().encode());

        assert_eq!(decoded.as_ref(), Ok(font.metrics()));
    }
}

#[test]
fn metrics_written_by_hand_map_and_kern_as_they_say() {
    // Catches a decode that reads the fields of a group or a pair in the wrong order, so a character draws another's
    // glyph or the wrong pair kerns: the hand-written bytes below are the format, written apart from `encode`.
    let bytes = written(&[(0x41, 0x42, 1)], &[(1, 2, -40)]);

    let Ok(metrics) = Metrics::decode(&bytes) else {
        panic!("the bytes are metrics");
    };

    assert_eq!(
        [metrics.glyph('A'), metrics.glyph('B'), metrics.glyph('C')],
        [Some(GlyphId(1)), Some(GlyphId(2)), None]
    );
    assert_eq!([metrics.kerning(GlyphId(1), GlyphId(2)), metrics.kerning(GlyphId(2), GlyphId(1))], [-40, 0]);
    assert_eq!(metrics.line_metrics().line_height(), 1000);
}

#[test]
fn metrics_out_of_order_or_naming_what_is_not_there_are_refused() {
    // Catches a decode that takes on trust what lookups rely on: groups out of order or overlapping, which the
    // character map's search by halves would miss characters in; a group mapping past the font's glyphs, whose
    // advance would read as none; pairs out of order or listed twice, which the kerning's search would miss; and bytes
    // left over, which say the bytes were not these metrics.
    let refused =
        |groups: &[(u32, u32, u32)], pairs: &[(u16, u16, i16)]| Metrics::decode(&written(groups, pairs)).err();
    let characters = Some(MetricsMalformed::Characters);
    let kerning = Some(MetricsMalformed::Kerning);

    assert_eq!(refused(&[(0x50, 0x50, 1), (0x41, 0x41, 2)], &[]), characters, "out of order");
    assert_eq!(refused(&[(0x41, 0x43, 0), (0x43, 0x44, 1)], &[]), characters, "overlapping");
    assert_eq!(refused(&[(0x41, 0x43, 1)], &[]), characters, "a glyph past the font's");
    assert_eq!(refused(&[(0x41, 0x40, 1)], &[]), characters, "ending before it starts");
    assert_eq!(refused(&[(0x11_0000, 0x11_0000, 1)], &[]), characters, "past Unicode");
    assert_eq!(refused(&[], &[(1, 2, -40), (1, 1, 10)]), kerning, "out of order");
    assert_eq!(refused(&[], &[(1, 2, -40), (1, 2, 10)]), kerning, "twice");

    let mut long = written(&[], &[]);
    long.push(0);
    assert_eq!(Metrics::decode(&long).err(), Some(MetricsMalformed::Trailing));
    let mut unitless = written(&[], &[]);
    unitless[..2].copy_from_slice(&0_u16.to_le_bytes());
    assert_eq!(Metrics::decode(&unitless).err(), Some(MetricsMalformed::Units));
}

#[test]
fn metrics_cut_short_or_counting_past_their_bytes_are_refused_never_a_panic() {
    // Catches a read past the end that is not checked, and a count taken on trust: a blob cut short would panic the
    // actor measuring with it, and a count of billions would allocate before a byte of the list was read.
    let encoded = noto().metrics().encode();
    let cuts = (0..encoded.len()).step_by(997).chain([encoded.len() - 1]);
    let short = cuts.clone().filter(|&length| Metrics::decode(&encoded[..length]).is_err()).count();
    assert_eq!(short, cuts.count());

    let mut counted = written(&[], &[]);
    // The advances' count, after the units per em and the line spacing.
    counted[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(Metrics::decode(&counted).err(), Some(MetricsMalformed::CutShort));
}

#[test]
fn garbled_metrics_are_refused_or_read_never_a_panic() {
    // Catches a value read from the bytes and used unchecked: a glyph number that overflows as a character is mapped,
    // or a class row that indexes past its adjustments. Whatever reads is shaped and kerned, and must not panic.
    let encoded = noto().metrics().encode();
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..64 {
        let mut garbled = encoded.clone();
        for _ in 0..6 {
            let at = usize::try_from(next() % encoded.len() as u64).expect("fits");
            garbled[at] = garbled[at].wrapping_add(u8::try_from(next() % 255 + 1).expect("fits"));
        }
        if let Ok(metrics) = Metrics::decode(&garbled) {
            let shaped = metrics.shape("Sphinx of black quartz, judge my vow.\nAV To \u{e9}");
            assert_eq!(shaped.lines.len(), 2);
        }
    }
}
