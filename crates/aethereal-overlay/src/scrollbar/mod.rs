//! The console's scroll bar (ADR-0053: The band and the console): a thin track down the right edge of the console's
//! panel, beside the scrollback's rows, and a thumb on it as tall as the share of the rows kept that are in view, and
//! as far up it as the view is scrolled up. Hidden while every row kept fits the view.

#[cfg(test)]
mod tests;

use crate::console::Scrolled;
use aethereal_math::{Rect2, Vec2};

/// The fewest rows' height the thumb is drawn, so a scrollback of thousands of rows still has a thumb to see.
const THUMB_LEAST_ROWS: f32 = 2.0;

/// Where the view stands, as the thumb shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// On the newest rows.
    Newest,
    /// Scrolled up from them: newer rows are below.
    Scrolled,
}

/// The scroll bar as it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bar {
    /// Every row kept fits the view: nothing is drawn.
    Hidden,
    /// The track, the thumb on it, and where the view stands.
    Shown { track: Rect2, thumb: Rect2, standing: Standing },
}

/// The bar on `track`, for a view of `shown` rows, each `row_pixels` tall, standing as `scrolled` says: the thumb's
/// height the track's share of `shown` over the rows kept, at least [`THUMB_LEAST_ROWS`] rows and at most the track;
/// at the track's foot on the newest rows and at its top on the oldest.
#[must_use]
pub fn bar(track: Rect2, row_pixels: f32, shown: usize, scrolled: Scrolled) -> Bar {
    let fits = scrolled.kept <= shown;
    if fits {
        return Bar::Hidden;
    }
    let furthest = scrolled.kept - shown;
    let up = scrolled.up.min(furthest);
    // Counts of rows, at most some thousands, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let (share, below) = (shown as f32 / scrolled.kept as f32, (furthest - up) as f32 / furthest as f32);
    let tall = (track.height() * share).max(THUMB_LEAST_ROWS * row_pixels).min(track.height());
    let top = (track.height() - tall).mul_add(below, track.min.y);
    let thumb = Rect2 { min: Vec2::new(track.min.x, top), max: Vec2::new(track.max.x, top + tall) };
    let standing = if up == 0 {
        Standing::Newest
    } else {
        Standing::Scrolled
    };

    Bar::Shown { track, thumb, standing }
}
