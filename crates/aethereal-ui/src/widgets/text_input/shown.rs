//! What a text input shows of its text: the text wrapped at its content box's width, as the renderer lays it out, as
//! many of its lines as the box is tall, from the first it shows, which follows the caret so the caret is always
//! among them. A press, a line's start and end, and a move up or down are found here, among the lines as drawn.

use crate::fonts::{Measuring, Unmeasured};
use crate::style::Style;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::{CaretLine, Carets, Colour, Font, Wrap};

/// How a text input's text is set, as its style and its screen's scale say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Set {
    pub font: Font,
    pub size: f32,
    pub colour: Colour,
}

impl Set {
    /// The text of `style` at `scale`.
    pub fn of(style: &Style, scale: f32) -> Self {
        Self { font: style.font, size: style.font_size * scale, colour: style.colour }
    }

    /// How a text is wrapped in a box `width` pixels wide.
    pub fn wrap(width: f32) -> Wrap {
        Wrap::At { width: width.max(f32::MIN_POSITIVE) }
    }
}

/// A text laid out in a text input's content box: its lines as drawn, the first it shows, how many it shows, and the
/// box.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub carets: Carets,
    pub top: usize,
    pub rows: usize,
    pub content: Rect2,
}

/// The first of `count` lines shown, `rows` of them at once, with the caret on the line `caret`: the first kept, `kept`,
/// no further down than the last lines fill the rows, and moved as little as shows the caret's.
pub fn fit(kept: usize, caret: usize, rows: usize, count: usize) -> usize {
    let top = kept.min(count.saturating_sub(rows));
    if caret < top {
        return caret;
    }
    if caret >= top + rows {
        return caret + 1 - rows;
    }

    top
}

impl Shown {
    /// `text` laid out as `set` sets it in `content`, the first line shown kept at `kept` and moved to show the caret
    /// at the byte `caret`.
    ///
    /// # Errors
    ///
    /// As [`Measuring::carets`]'s, for want of the font's metrics.
    pub fn of(
        set: Set,
        measuring: &mut Measuring<'_>,
        text: &str,
        content: Rect2,
        (kept, caret): (u32, usize),
    ) -> Result<Self, Unmeasured> {
        let carets = measuring.carets(set.font, set.size, text, Set::wrap(content.width()))?;
        // A box is some thousands of pixels tall and a line some tens, so the rows it shows count far fewer than an
        // f32 holds exactly.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = (content.height() / carets.line_height.max(f32::MIN_POSITIVE)).round().max(1.0) as usize;
        let kept = usize::try_from(kept).unwrap_or(usize::MAX);
        let top = fit(kept, carets.line_of(caret), rows, carets.lines.len());

        Ok(Self { carets, top, rows, content })
    }

    /// The first line it shows, as a text input's state keeps it.
    pub fn kept_top(&self) -> u32 {
        u32::try_from(self.top).unwrap_or(u32::MAX)
    }

    /// The top of the line `line`, in the coordinates of its content box: above the box for a line before the first
    /// shown.
    pub fn y_of(&self, line: usize) -> f32 {
        // A text input's lines count far fewer than an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let below = line as f32 - self.top as f32;

        below.mul_add(self.carets.line_height, self.content.min.y)
    }

    /// The lines it shows, each with its number.
    pub fn visible(&self) -> impl Iterator<Item = (usize, &CaretLine)> {
        self.carets.lines.iter().enumerate().skip(self.top).take(self.rows)
    }

    /// The line a caret at the byte `at` stands on.
    pub fn line_at(&self, at: usize) -> &CaretLine {
        &self.carets.lines[self.carets.line_of(at)]
    }

    /// The caret's place nearest the pointer at `at`: on the line under it, or the first or last line when it is above
    /// or below them all, as a drag past the box reaches the lines it does not show.
    pub fn pressed(&self, at: Vec2) -> usize {
        let down = ((at.y - self.content.min.y) / self.carets.line_height.max(f32::MIN_POSITIVE)).floor();
        let last = self.carets.lines.len().saturating_sub(1);
        // A text input's lines count far fewer than an f32 holds exactly, and a pointer far past them is clamped.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
        let line = (self.top as f32 + down).clamp(0.0, last as f32) as usize;

        self.carets.lines[line].nearest(at.x - self.content.min.x)
    }
}
