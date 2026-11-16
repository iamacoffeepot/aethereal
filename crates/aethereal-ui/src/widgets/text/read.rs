//! A text as its reader reads it (ADR-0056: Copy and paste), for `Text`, `Rich`, and `Selectable` alike: its runs'
//! text joined, which its screen's selection counts offsets in; the boundary nearest a place in it as it is drawn; and
//! the part of it its screen's selection holds, drawn beneath it.

use super::super::rich::Flow;
use super::super::selectable::SELECTION_STRENGTH;
use super::{Width, fitted, rim};
use crate::fonts::{Measuring, Unmeasured};
use crate::style::Style;
use crate::widget::{Draws, Laid, Quad, Sample, Selected};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::{Align, CaretLine, Carets, Colour, Run, Wrap};

/// `runs`' text, joined: what a selection's offsets count and a copy writes.
pub fn joined(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

/// A text as drawn in its content box: its lines, where the first starts, and the width its lines are set within.
struct Shown {
    carets: Carets,
    origin: Vec2,
    within: f32,
    align: Align,
}

impl Shown {
    /// `runs`, flowing as `flow` says, as drawn in `laid`'s content box styled `style`: laid out as `Rich` lays them
    /// out, wrapped where they wrap.
    fn of(
        runs: &[Run],
        flow: Flow,
        style: &Style,
        laid: &Laid,
        measuring: &mut Measuring<'_>,
    ) -> Result<Self, Unmeasured> {
        let pad = rim(style, laid.scale);
        let width = pad.mul_add(-2.0, laid.content.width());
        let offered = match flow {
            Flow::Wraps => Width::Within(width),
            Flow::Unbroken => Width::Unbounded,
        };
        let (measured, wrap) = fitted(runs, style, measuring, offered)?;
        let within = match wrap {
            Wrap::None => measured.width,
            Wrap::At { width } => width,
        };
        let shift = match style.text_align {
            Align::Left => 0.0,
            Align::Centre => (width - within) / 2.0,
            Align::Right => width - within,
        };
        let carets = measuring.carets(style.font, style.font_size * laid.scale, &joined(runs), wrap)?;

        Ok(Self { carets, origin: laid.content.min + Vec2::new(pad + shift, pad), within, align: style.text_align })
    }

    /// How far right of its origin the line `line` starts, as it is aligned.
    fn shift(&self, line: &CaretLine) -> f32 {
        match self.align {
            Align::Left => 0.0,
            Align::Centre => (self.within - line.width) / 2.0,
            Align::Right => self.within - line.width,
        }
    }

    /// The boundary nearest the pointer at `at`: on the line under it, or the first or last line past them.
    fn pressed(&self, at: Vec2) -> usize {
        let height = self.carets.line_height.max(f32::MIN_POSITIVE);
        let last = self.carets.lines.len().saturating_sub(1);
        // A text's lines count far fewer than an f32 holds exactly, and a pointer far past them is clamped.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
        let number = ((at.y - self.origin.y) / height).floor().clamp(0.0, last as f32) as usize;
        let Some(line) = self.carets.lines.get(number) else {
            return 0;
        };

        line.nearest(at.x - self.origin.x - self.shift(line))
    }
}

/// The boundary of `runs`' text nearest `at`, as they are drawn flowing as `flow` says in `laid`, styled `style`:
/// nothing before its font's metrics come.
pub fn boundary(
    runs: &[Run],
    flow: Flow,
    at: Vec2,
    (style, laid): (&Style, &Laid),
    measuring: &mut Measuring<'_>,
) -> u32 {
    let Ok(shown) = Shown::of(runs, flow, style, laid, measuring) else {
        return 0;
    };

    u32::try_from(shown.pressed(at)).unwrap_or(u32::MAX)
}

/// Draws the part of `runs`' text the screen's selection holds, as `draws` says, beneath the text, which flows as `flow`
/// says in `laid` styled `style`: a quad over the part of each line it covers.
pub fn highlight(runs: &[Run], flow: Flow, (style, laid): (&Style, &Laid), draws: &mut Draws<'_, '_>) {
    let Selected::Part { start, end, colour } = draws.selected() else {
        return;
    };
    let Ok(shown) = Shown::of(runs, flow, style, laid, draws.measuring()) else {
        return;
    };
    let at = |offset: u32| usize::try_from(offset).unwrap_or(usize::MAX);
    let (start, end) = (at(start), at(end));
    let colour = Colour { alpha: colour.alpha * SELECTION_STRENGTH, ..colour };
    let clip = laid.clip.intersection(laid.content);
    let height = shown.carets.line_height;
    for (number, line) in shown.carets.lines.iter().enumerate() {
        let (from, to) = (start.max(line.start), end.min(line.end));
        if from >= to {
            continue;
        }
        let left = shown.origin.x + shown.shift(line);
        // A text's lines count far fewer than an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let top = (number as f32).mul_add(height, shown.origin.y);
        let rect =
            Rect2 { min: Vec2::new(left + line.x_of(from), top), max: Vec2::new(left + line.x_of(to), top + height) };
        draws.quad(Quad {
            rect,
            clip,
            colour,
            border_width: 0.0,
            border_colour: colour,
            radius: 0.0,
            sample: Sample::Solid,
        });
    }
}
