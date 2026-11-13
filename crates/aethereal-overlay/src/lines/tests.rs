use super::figures::count;
use super::{FrameLines, rows};
use crate::chart::tests::read;
use crate::colour::STONE;
use crate::frames::Slide;
use crate::layout::Scale;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameDrawn;

/// The rows' box on a window 1080 pixels tall at a scale of one: as wide as the frame chart, and three lines tall.
const AREA: Rect2 = Rect2 { min: Vec2::new(10.0, 170.0), max: Vec2::new(966.0, 224.6) };

/// The row `index` from the top: a third of the box, its bars three pixels in from its top and foot.
fn row(index: u8) -> Rect2 {
    let tall = AREA.height() / 3.0;
    let top = f32::from(index).mul_add(tall, AREA.min.y);

    Rect2 { min: Vec2::new(AREA.min.x, top + 3.0), max: Vec2::new(AREA.max.x, top + tall - 3.0) }
}

/// What a frame that recorded `draws`, drew no triangles, and uploaded fifteen bytes drew.
const fn frame(draws: u32) -> FrameDrawn {
    FrameDrawn { draws, triangles: 0, uploaded_bytes: 15 }
}

/// Each column of a row is the most of its frames, its bar against the most any column shown came to, from the row's
/// foot; and a line that is nothing in every column draws nothing. So a column that averages its frames and hides a
/// spike, a row scaled against something else than what it shows, or an empty row drawn as a strip of one-pixel bars,
/// is caught.
#[test]
fn a_row_s_columns_are_the_most_of_their_frames_against_the_most_shown() {
    let second = 1_000_000_000;
    let mut lines = FrameLines::default();
    for (elapsed_nanos, draws) in [(second / 10, 10), (second / 5, 30), (second * 11 / 10, 15)] {
        lines.push(elapsed_nanos, frame(draws));
    }
    let slide = Slide { newest: 1, through: 0.5, span_nanos: second };
    let mut fills = Vec::new();
    lines.cut(60);

    rows(&mut fills, AREA, Scale(1.0), slide, &lines.columns());

    let drawn = read(&fills);
    let (draws_row, uploaded_row) = (row(0), row(2));
    let heights: Vec<f32> = drawn.iter().map(|record| record.area[3]).collect();
    let expected = [draws_row.height() / 2.0, draws_row.height(), uploaded_row.height(), uploaded_row.height()];
    let near = heights.len() == expected.len() && heights.iter().zip(expected).all(|(a, b)| (a - b).abs() < 0.01);
    assert!(near, "{heights:?} are not {expected:?}");
    assert!(drawn.iter().all(|record| record.colour == STONE.at(0.6)));
    assert!(
        (drawn[0].area[1] + drawn[0].area[3] - draws_row.max.y).abs() < 0.01,
        "{:?} is off the row's foot",
        drawn[0]
    );
    assert!(
        (drawn[3].area[1] + drawn[3].area[3] - uploaded_row.max.y).abs() < 0.01,
        "{:?} is off the row's foot",
        drawn[3]
    );
    let runs = lines.runs();
    assert_eq!(runs.last().map(|run| run.text.trim()), Some("15 B"));
}

/// Counts are whole to ten thousand and then in thousands and millions, so a count rounded to a unit that hides its
/// last digits too soon, or one that never climbs past thousands, is caught.
#[test]
fn counts_are_whole_to_ten_thousand_then_in_thousands_and_millions() {
    assert_eq!([count(9_999), count(12_345), count(4_560_000)], ["9999", "12.3k", "4.6M"]);
}
