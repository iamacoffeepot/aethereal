//! The atlas's bookkeeping: fields packed apart on shelves, the atlas grown when one does not fit, and packed again
//! from nothing when it can grow no further, the glyphs drawn longest ago dropped first.

use super::shelf::Shelves;
use super::*;

/// The glyph numbered `glyph` of the renderer's own proportional font.
const fn key(glyph: u16) -> GlyphKey {
    GlyphKey { font: FontKey::Sans, glyph: GlyphId(glyph) }
}

/// A field `width` by `height` texels.
const fn field(width: u32, height: u32) -> Field {
    Field { width, height, left: 0, top: 0 }
}

/// Whether the boxes `a` and `b`, each a top left, a width, and a height, share a texel.
const fn overlap(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> bool {
    let apart_x = a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0;
    let apart_y = a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
    !(apart_x || apart_y)
}

#[test]
fn shelves_pack_boxes_apart_within_the_square_and_refuse_one_with_no_room() {
    // Catches a shelf that takes a box taller than itself, a box set past the square's edge, and two boxes on one
    // texel.
    let mut shelves = Shelves::new(32);
    let sizes = [(10, 12), (10, 8), (10, 12), (10, 12), (20, 10), (8, 8), (24, 6)];
    let mut placed = Vec::new();
    for (width, height) in sizes {
        let Some((x, y)) = shelves.place(width, height) else {
            panic!("{width}x{height} fits");
        };
        assert!(x + width <= 32 && y + height <= 32, "{width}x{height} at {x},{y} is inside");
        placed.push((x, y, width, height));
    }
    for (at, a) in placed.iter().enumerate() {
        for b in &placed[at + 1..] {
            assert!(!overlap(*a, *b), "{a:?} and {b:?} share texels");
        }
    }

    assert_eq!(shelves.place(10, 10), None, "no shelf or room below is left for it");
    assert_eq!(shelves.place(33, 1), None, "wider than the square");
}

#[test]
fn glyphs_fit_with_a_gap_and_the_atlas_doubles_when_one_does_not_up_to_its_longest() {
    // Catches fields packed edge to edge, which filtering bleeds between, an atlas that refuses a glyph where it could
    // grow, and one that grows past the GPU's longest side.
    let mut atlas = Atlas::new(2048);
    assert_eq!(atlas.side(), FIRST_SIDE);
    let first = atlas.add(&[(key(1), field(40, 50)), (key(2), field(40, 50))], &HashMap::new(), 0);
    assert_eq!(first.arranged, Arranged::Kept);
    let (Some(one), Some(two)) = (atlas.slot(key(1)), atlas.slot(key(2))) else {
        panic!("both are in the atlas");
    };
    assert!(!overlap((one.x, one.y, 40 + GAP, 50 + GAP), (two.x, two.y, 40, 50)), "a texel apart");

    let big: Vec<(GlyphKey, Field)> = (10..40).map(|glyph| (key(glyph), field(200, 200))).collect();
    let grown = atlas.add(&big, &HashMap::new(), 0);
    assert_eq!(grown.arranged, Arranged::Grown { side: 2048 });
    assert!(grown.unfit.is_empty() && grown.placed.len() == 30);
    assert_eq!(atlas.slot(key(1)), Some(one), "growing moves nothing");
    assert_eq!(atlas.packings(), 0);
}

#[test]
fn a_full_atlas_is_packed_again_keeping_the_glyphs_drawn_last_and_dropping_those_drawn_longest_ago() {
    // Catches a full atlas that drops the glyphs on screen now, or the newest rather than the stalest, and one that
    // keeps a glyph where it lay while another is packed over it; and the texels it counts as used left counting the
    // glyphs it dropped, or the new glyphs it took back out before packing again (ADR-0053).
    let mut atlas = Atlas::new(100);
    // Four fields of 49 texels a side, a texel's gap each, fill the atlas.
    let quarter = field(49, 49);
    let filled =
        atlas.add(&[(key(1), quarter), (key(2), quarter), (key(3), quarter), (key(4), quarter)], &HashMap::new(), 0);
    assert!(filled.unfit.is_empty());
    assert_eq!(atlas.used_texels(), 4 * 49 * 49);
    let drawn = HashMap::from([(key(1), 9), (key(2), 3), (key(3), 7), (key(4), 9)]);

    let added = atlas.add(&[(key(5), quarter), (key(6), quarter)], &drawn, 9);
    let Arranged::Packed { side, moves } = &added.arranged else {
        panic!("the atlas is packed again: {:?}", added.arranged);
    };
    assert_eq!(*side, 100);
    assert_eq!(atlas.packings(), 1);
    assert!(added.unfit.is_empty(), "the two new glyphs fit");
    let held: Vec<bool> = (1..=6).map(|glyph| atlas.slot(key(glyph)).is_some()).collect();
    assert_eq!(held, [true, false, false, true, true, true], "the two drawn at 9 kept; 2 and 3, drawn before, dropped");
    assert_eq!(moves.len(), 2, "each glyph kept is copied to where it lies now");
    assert_eq!(atlas.used_texels(), 4 * 49 * 49, "the four held now, and none of those dropped");

    let slots: Vec<(u32, u32, u32, u32)> = [1, 4, 5, 6]
        .iter()
        .filter_map(|glyph| atlas.slot(key(*glyph)))
        .map(|slot| (slot.x, slot.y, slot.field.width, slot.field.height))
        .collect();
    for (at, a) in slots.iter().enumerate() {
        for b in &slots[at + 1..] {
            assert!(!overlap(*a, *b), "{a:?} and {b:?} share texels");
        }
    }
}

#[test]
fn a_glyph_that_fits_nowhere_even_packed_again_is_unfit_and_a_space_takes_no_room() {
    // Catches a glyph larger than the longest side that loops growing, or is placed off the texture; and a space's
    // empty field packed, taking a shelf.
    let mut atlas = Atlas::new(64);
    let added = atlas.add(&[(key(1), field(0, 0)), (key(2), field(80, 10))], &HashMap::new(), 0);

    assert_eq!(added.unfit, vec![key(2)]);
    assert_eq!(atlas.slot(key(1)).map(|slot| slot.field.is_empty()), Some(true));
    assert_eq!(atlas.slot(key(2)), None);
}
