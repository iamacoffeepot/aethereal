//! The scroll bar's tests: the thumb's height, its least height, and its place along the track, and the bar hidden
//! while every row fits.

use super::{Bar, Standing, bar};
use crate::console::Scrolled;
use aethereal_math::{Rect2, Vec2};

/// A track 4 pixels wide and 200 tall, from 100 down.
const TRACK: Rect2 = Rect2 { min: Vec2::new(10.0, 100.0), max: Vec2::new(14.0, 300.0) };

/// Rows 20 pixels tall, as the track holds ten of.
const ROW_PIXELS: f32 = 20.0;

/// The thumb on `TRACK` for a view of ten rows standing as `scrolled` says, with where the view stands, or none when
/// the bar is hidden.
fn thumb(scrolled: Scrolled) -> Option<(Rect2, Standing)> {
    match bar(TRACK, ROW_PIXELS, 10, scrolled) {
        Bar::Hidden => None,
        Bar::Shown { thumb, standing, .. } => Some((thumb, standing)),
    }
}

/// Forty rows kept, ten in view: the thumb is a quarter of the track, at its foot on the newest rows, at its top on the
/// oldest, and halfway between them halfway up, so a thumb sized by the wrong share, or one that runs the wrong way,
/// is caught.
#[test]
fn the_thumb_is_the_share_in_view_and_stands_where_the_view_does() {
    let at = |up| thumb(Scrolled { kept: 40, up }).map(|(thumb, standing)| (thumb.min.y, thumb.max.y, standing));

    assert_eq!(at(0), Some((250.0, 300.0, Standing::Newest)));
    assert_eq!(at(15), Some((175.0, 225.0, Standing::Scrolled)));
    assert_eq!(at(30), Some((100.0, 150.0, Standing::Scrolled)));
    assert_eq!(at(31), at(30), "a view up past the oldest row stands at the top");
}

/// Two thousand rows kept would make a thumb a pixel tall; it is held at two rows' height instead, and still reaches
/// both ends of the track, so a thumb too small to see, or one that overruns the track, is caught.
#[test]
fn a_long_scrollback_keeps_a_thumb_two_rows_tall() {
    let newest = thumb(Scrolled { kept: 2_000, up: 0 }).map(|(thumb, _)| (thumb.min.y, thumb.max.y));
    let oldest = thumb(Scrolled { kept: 2_000, up: 1_990 }).map(|(thumb, _)| (thumb.min.y, thumb.max.y));

    assert_eq!(newest, Some((260.0, 300.0)));
    assert_eq!(oldest, Some((100.0, 140.0)));
}

/// Ten rows kept fit a view of ten, and none needs a bar; eleven do not, so a bar drawn over a console with nothing to
/// scroll, or one missing when a row is hidden, is caught.
#[test]
fn the_bar_hides_while_every_row_fits() {
    assert_eq!(bar(TRACK, ROW_PIXELS, 10, Scrolled { kept: 10, up: 0 }), Bar::Hidden);
    assert!(thumb(Scrolled { kept: 11, up: 0 }).is_some());
}
