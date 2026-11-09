//! Tables the fixture font does not have, or forms of them it does not use, built by hand: a format 12 character map
//! with an astral character, the older `kern` table, a composite glyph that holds itself, and a font with no tables.

use super::head::LocaOffsets;
use super::*;
use crate::font::Font;
use crate::font::kerning::Kerning;

/// Each of `values`, as 16 bits big-endian.
fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_be_bytes()).collect()
}

/// Each of `values`, as 32 bits big-endian.
fn longs(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_be_bytes()).collect()
}

#[test]
fn a_full_character_map_is_preferred_and_reaches_past_the_basic_plane() {
    // Catches the format 4 subtable read where the font has a format 12 one, which loses every emoji and the
    // historic scripts, or a format 12 group whose glyphs run past the font's mapped to glyphs it has not.
    let records = [words(&[0, 2, 3, 1]), longs(&[20]), words(&[3, 10]), longs(&[52])].concat();
    let basic = words(&[4, 32, 0, 4, 4, 1, 0, 0x41, 0xFFFF, 0, 0x41, 0xFFFF, 1u16.wrapping_sub(0x41), 1, 0, 0]);
    let full = [words(&[12, 0]), longs(&[40, 0, 2, 0x41, 0x43, 5, 0x1_F600, 0x1_F602, 10])].concat();
    let table = [records, basic, full].concat();

    let map = cmap::read(Data::new(&table, Table::Cmap), 12).expect("the map reads");
    let glyphs = [0x40, 0x41, 0x43, 0x1_F600, 0x1_F601, 0x1_F602].map(|character| map.glyph(character));

    assert_eq!(glyphs, [None, Some(5), Some(7), Some(10), Some(11), None]);
}

#[test]
fn the_kern_table_kerns_its_horizontal_pairs_and_not_its_cross_stream_ones() {
    // Catches a `kern` pair read from the wrong place, or a cross-stream subtable, which moves glyphs up and down,
    // read as kerning.
    let horizontal = words(&[0, 26, 0x0001, 2, 12, 1, 0, 1, 2, (-50_i16).cast_unsigned(), 1, 3, 30]);
    let cross_stream = words(&[0, 20, 0x0005, 1, 6, 0, 0, 2, 1, 99]);
    let table = [words(&[0, 2]), horizontal, cross_stream].concat();

    let pairs = kern::read(Data::new(&table, Table::Kern)).expect("the table reads");
    let kerning = Kerning::from_tables(None, Some(pairs));

    assert_eq!([kerning.between(1, 2), kerning.between(1, 3), kerning.between(2, 1)], [-50, 30, 0]);
}

#[test]
fn a_composite_glyph_that_holds_itself_is_refused() {
    // Catches composites resolved without a limit on their nesting: a glyph made of itself would loop, or recurse
    // until the stack overflowed, as the font was read.
    let loca = words(&[0, 0, 8]);
    let glyf = words(&[0xFFFF, 0, 0, 0, 0, 0x0002, 1, 0]);

    let read = glyf::read(Data::new(&loca, Table::Loca), Data::new(&glyf, Table::Glyf), LocaOffsets::Short, 2);

    assert_eq!(read.err(), Some(FontRefused::Malformed(Table::Glyf)));
}

#[test]
fn a_font_missing_a_table_names_it() {
    // Catches a missing table read as empty, or refused without saying which, so its maker cannot tell what is wrong.
    let empty = words(&[1, 0, 0, 0, 0, 0]);

    assert_eq!(Font::read(&empty).err(), Some(FontRefused::Missing(Table::Head)));
}
