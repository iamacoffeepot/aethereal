//! The fields of a square and of the fixture font's glyphs: their sign, their corners, and how closely they draw the
//! outline back.

use super::colour::{Colour, colour, is_corner};
use super::correct::correct;
use super::sign::inside;
use super::*;
use crate::font::outline::{Contour, Segment};
use crate::font::tests::noto;
use crate::font::{Font, GlyphId};
use aethereal_math::Vec2;

/// A square `side` font units across, its corner at the origin, wound clockwise as TrueType winds an outer contour.
fn square(side: f32) -> Outline {
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(0.0, side), Vec2::new(side, side), Vec2::new(side, 0.0)];
    let segments = (0..4).map(|index| Segment::Line { from: corners[index], to: corners[(index + 1) % 4] }).collect();
    Outline { contours: vec![Contour { segments }] }
}

/// The glyph of `character` in `font`, and its outline.
fn outline(font: &Font, character: char) -> &Outline {
    font.metrics().glyph(character).and_then(|glyph| font.outline(glyph)).expect("the fixture draws the character")
}

/// Where `at`, in pixels from the glyph's origin, is in `field`'s texels from its top left.
#[allow(clippy::cast_possible_truncation, reason = "a point in a glyph's pixels")]
fn texel(field: &Msdf, at: Point) -> (f32, f32) {
    ((at.x - f64::from(field.left)) as f32, (f64::from(field.top) - at.y) as f32)
}

#[test]
fn an_o_reads_inside_on_its_ring_and_outside_in_its_hole_and_around_it() {
    // Catches a distance whose sign is turned, for a line or for a curve, or a field whose bytes are turned: the sign
    // correction would hide the first at texels' centres and leave its edges ragged, so the measured distances are
    // checked before it, and the field after.
    let font = noto();
    let ring = outline(&font, 'O');
    let scale = font.metrics().pixels_per_unit(BASE_PIXELS_PER_EM);
    let edges = colour(contours(ring, scale)).into_iter().flatten().collect::<Vec<_>>();
    let bounds = ring.bounds().expect("O has an outline");
    let centre = Point::new(f64::from(bounds.center().x * scale), f64::from(bounds.center().y * scale));

    let top = f64::from(bounds.max.y * scale);
    let stroke = (0..400)
        .map(|step| Point::new(centre.x, top - f64::from(step) * 0.05))
        .skip_while(|&at| !inside(&edges, at))
        .take_while(|&at| inside(&edges, at))
        .collect::<Vec<_>>();
    let on_ring = stroke[stroke.len() / 2];
    let around = Point::new(f64::from(bounds.min.x * scale) - 2.0, centre.y);

    let measured = [on_ring, centre, around].map(|at| sign::median(distances(&edges, at)) > 0.0);
    let field = generate(ring, scale, DEFAULT_PADDING);
    let read = [on_ring, centre, around].map(|at| {
        let (x, y) = texel(&field, at);
        field.median(x, y) > 0.5
    });

    assert_eq!(measured, [true, false, false]);
    assert_eq!(read, [true, false, false]);
}

#[test]
fn the_two_edges_at_every_corner_differ_in_colour() {
    // Catches colours switched wrongly at a corner, or a contour's last stretch left the colour of its first, which it
    // meets: that corner would be rounded, as a one-channel field rounds every corner.
    let font = noto();
    let printable =
        (' '..='~').filter_map(|character| font.metrics().glyph(character)).filter_map(|glyph| font.outline(glyph));

    let mut corners = 0;
    for outline in printable {
        for contour in colour(contours(outline, 1.0)) {
            for (index, after) in contour.iter().enumerate() {
                let before = contour[(index + contour.len() - 1) % contour.len()];
                let turning =
                    is_corner(before.edge.direction(1.0).normalized(), after.edge.direction(0.0).normalized());
                if turning {
                    corners += 1;
                    let distinct = before.colour != after.colour;
                    let coloured = before.colour != Colour::WHITE && after.colour != Colour::WHITE;
                    assert!(distinct && coloured, "{:?} meets {:?}", before.colour, after.colour);
                }
            }
        }
    }
    assert!(corners > 500, "only {corners} corners");
}

#[test]
fn a_square_corner_stays_sharp_between_texels() {
    // Catches a field whose channels do not keep a corner: a tenth of a pixel inside a square's corner, a one-channel
    // field sampled between texels already reads outside, so text would draw with rounded corners at every size.
    let field = generate(&square(16.0), 1.0, DEFAULT_PADDING);

    let corner = Point::new(16.0, 16.0);
    let just_inside = texel(&field, Point::new(corner.x - 0.1, corner.y - 0.1));
    let just_outside = texel(&field, Point::new(corner.x + 0.1, corner.y + 0.1));

    assert!(field.median(just_inside.0, just_inside.1) > 0.5);
    assert!(field.median(just_outside.0, just_outside.1) < 0.5);
}

#[test]
fn a_field_drawn_at_its_own_size_matches_the_outline() {
    // Catches anything that leaves the field wrong between texels' centres: a pseudo-distance taken on the wrong side,
    // edges coloured so that a channel clashes, or clashes left uncorrected. Each glyph is sampled sixteen times a
    // texel, between texels' centres as the glyph program samples it, and compared with the outline's own inside.
    let font = noto();
    let scale = font.metrics().pixels_per_unit(BASE_PIXELS_PER_EM);

    for character in ['g', 'W', '@', '\u{e9}', 'O', '&', 'A'] {
        let glyph = outline(&font, character);
        let edges = colour(contours(glyph, scale)).into_iter().flatten().collect::<Vec<_>>();
        let field = generate(glyph, scale, DEFAULT_PADDING);

        let samples = (0..field.height * 4).flat_map(|row| (0..field.width * 4).map(move |column| (column, row)));
        let (mut total, mut wrong) = (0_u32, 0_u32);
        for (column, row) in samples {
            let (x, y) = ((f64::from(column) + 0.5) / 4.0, (f64::from(row) + 0.5) / 4.0);
            let at = Point::new(f64::from(field.left) + x, f64::from(field.top) - y);
            #[allow(clippy::cast_possible_truncation, reason = "a texel's position in a glyph")]
            let drawn = field.median(x as f32, y as f32) > 0.5;
            total += 1;
            wrong += u32::from(drawn != inside(&edges, at));
        }

        let fraction = f64::from(wrong) / f64::from(total);
        assert!(fraction < 0.002, "{character:?}: {wrong} of {total} samples drawn wrong");
    }
}

#[test]
fn of_two_clashing_texels_the_one_further_from_the_edge_is_evened_out() {
    // Catches the channels' differences ordered the wrong way, or both texels of a clash evened out: corners would be
    // lost where no clash was, or false edges kept where one was. Red and green swap sides between the two texels, so
    // sampling between them would draw an edge; blue says the first is further inside.
    let grid = Grid { left: 0, top: 0, width: 3, height: 1 };
    let mut field = [[0.9, 0.2, 0.8], [0.2, 0.9, 0.75], [0.25, 0.85, 0.7]];

    correct(&mut field, grid, CLASH / 8.0);

    assert_eq!(field, [[0.8; 3], [0.2, 0.9, 0.75], [0.25, 0.85, 0.7]]);
}

#[test]
fn a_glyph_that_draws_nothing_has_an_empty_field() {
    // Catches a space measured as a field of padding alone, or the bounds of no edges taken as infinite.
    let font = noto();
    let space = font.metrics().glyph(' ').map(|glyph| font.msdf(glyph, BASE_PIXELS_PER_EM, DEFAULT_PADDING));

    assert_eq!(space.flatten().map(|field| (field.width, field.height, field.pixels.len())), Some((0, 0, 0)));
    assert!(font.msdf(GlyphId(u16::MAX), BASE_PIXELS_PER_EM, DEFAULT_PADDING).is_none());
}
