//! A glyph's outline: closed contours of line and quadratic segments, in font units, `y` up. TrueType keeps a contour
//! as points on and off its curve, two control points in a row implying an on-curve point midway between them; the
//! segments here have every such point made.

#[cfg(test)]
mod tests;

use aethereal_math::{Rect2, Vec2};

/// A glyph's outline: its contours, each closed. A TrueType font winds an outer contour clockwise and a hole
/// anticlockwise, and fills by the nonzero rule.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outline {
    pub contours: Vec<Contour>,
}

/// One closed contour: its segments in order, each starting where the one before it ends, the last ending where the
/// first starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Contour {
    pub segments: Vec<Segment>,
}

/// A piece of a contour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// A straight line.
    Line { from: Vec2, to: Vec2 },
    /// A quadratic Bézier curve, which leaves `from` towards `control` and arrives at `to` from it.
    Quadratic { from: Vec2, control: Vec2, to: Vec2 },
}

/// A point of a TrueType contour: on the curve, or a control point between two that are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: Vec2,
    pub on_curve: bool,
}

/// What a contour's walk waits for next.
#[derive(Debug, Clone, Copy)]
enum Waiting {
    /// A point to draw a line to, from the pen.
    Point,
    /// The end of a curve whose control point is this.
    End(Vec2),
}

impl Outline {
    /// The outline of `contours` of TrueType points. A contour of fewer than two points draws nothing, and is left out.
    #[must_use]
    pub fn from_points(contours: &[Vec<Point>]) -> Self {
        let contours = contours
            .iter()
            .map(|points| Contour::from_points(points))
            .filter(|contour| !contour.segments.is_empty())
            .collect();
        Self { contours }
    }

    /// The smallest box holding the outline, curves bounded where they turn, not by their control points. `None` for
    /// an outline of no contours.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect2> {
        let mut segments = self.contours.iter().flat_map(|contour| &contour.segments);
        let first = segments.next()?.bounds();
        Some(segments.fold(first, |bounds, segment| bounds.union(segment.bounds())))
    }
}

impl Contour {
    /// The contour through TrueType `points`, closed: a control point between two others implies an on-curve point
    /// midway between them, and a contour of only control points starts at the one implied between its last and
    /// first. Fewer than two points make no contour.
    #[must_use]
    pub fn from_points(points: &[Point]) -> Self {
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return Self::default();
        };
        if points.len() < 2 {
            return Self::default();
        }

        let first_on = points.iter().position(|point| point.on_curve);
        let (start, walk) = first_on.map_or_else(
            || {
                let implied = Point { at: midpoint(last.at, first.at), on_curve: true };
                (implied.at, [points, &[implied]].concat())
            },
            |index| {
                let (before, after) = points.split_at(index + 1);
                (points[index].at, [after, before].concat())
            },
        );

        let mut pen = start;
        let mut waiting = Waiting::Point;
        let mut segments = Vec::with_capacity(walk.len());
        for point in walk {
            match (waiting, point.on_curve) {
                (Waiting::Point, true) => {
                    segments.push(Segment::Line { from: pen, to: point.at });
                    pen = point.at;
                }
                (Waiting::Point, false) => waiting = Waiting::End(point.at),
                (Waiting::End(control), true) => {
                    segments.push(Segment::Quadratic { from: pen, control, to: point.at });
                    pen = point.at;
                    waiting = Waiting::Point;
                }
                (Waiting::End(control), false) => {
                    let implied = midpoint(control, point.at);
                    segments.push(Segment::Quadratic { from: pen, control, to: implied });
                    pen = implied;
                    waiting = Waiting::End(point.at);
                }
            }
        }

        Self { segments }
    }
}

impl Segment {
    /// Where the segment starts.
    pub const fn from(self) -> Vec2 {
        match self {
            Self::Line { from, .. } | Self::Quadratic { from, .. } => from,
        }
    }

    /// Where the segment ends.
    pub const fn to(self) -> Vec2 {
        match self {
            Self::Line { to, .. } | Self::Quadratic { to, .. } => to,
        }
    }

    /// The segment with every point mapped by `map`.
    #[must_use]
    pub fn map(self, map: impl Fn(Vec2) -> Vec2) -> Self {
        match self {
            Self::Line { from, to } => Self::Line { from: map(from), to: map(to) },
            Self::Quadratic { from, control, to } => {
                Self::Quadratic { from: map(from), control: map(control), to: map(to) }
            }
        }
    }

    /// The smallest box holding the segment: a curve's turning point counted, not its control point.
    pub fn bounds(self) -> Rect2 {
        match self {
            Self::Line { from, to } => Rect2::from_points(&[from, to]),
            Self::Quadratic { from, control, to } => {
                let turn = |from: f32, control: f32, to: f32| {
                    let bend = 2.0f32.mul_add(-control, from) + to;
                    let amount = (from - control) / bend;
                    let inside = bend != 0.0 && amount > 0.0 && amount < 1.0;
                    if inside {
                        amount
                    } else {
                        0.0
                    }
                };
                let at = |amount: f32| quadratic(from, control, to, amount);
                let x_turn = at(turn(from.x, control.x, to.x));
                let y_turn = at(turn(from.y, control.y, to.y));
                Rect2::from_points(&[from, to, x_turn, y_turn])
            }
        }
    }
}

/// The point midway between `a` and `b`.
fn midpoint(a: Vec2, b: Vec2) -> Vec2 {
    (a + b) * 0.5
}

/// The point `amount` along the quadratic curve from `from` through `control` to `to`.
fn quadratic(from: Vec2, control: Vec2, to: Vec2, amount: f32) -> Vec2 {
    let rest = 1.0 - amount;
    from * (rest * rest) + control * (2.0 * rest * amount) + to * (amount * amount)
}
