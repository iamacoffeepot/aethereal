use core::num::NonZeroU16;

use crate::Vec2;

/// A centripetal Catmull–Rom curve through points, borrowed, sampled at `steps` a span.
///
/// The curve passes through every point in order; span `i` runs from point `i` to point `i + 1`.
/// Each span is shaped by the points either side of it, with knots spaced by the square root of the
/// distance between neighbouring points (the centripetal parameterisation), which keeps a span from
/// looping or forming a cusp where its points bunch up. The first and last spans have a neighbour
/// on one side only, so the missing one is the next point in from that end reflected through the
/// end: the curve leaves its first point along the first span and arrives at its last along the
/// last.
///
/// A point repeated in a row makes a span of zero length, which stays at that point; the spans
/// either side of it treat the repeat as an end, as if their neighbour there were reflected.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct CatmullRom<'a> {
    /// The points the curve passes through, in order.
    pub points: &'a [Vec2],
    /// How many samples each span gives, counting its start and not its end.
    pub steps: NonZeroU16,
}

/// The samples of a curve in order, from [`CatmullRom::samples`].
#[derive(Debug, Clone)]
#[must_use]
pub struct CatmullRomSamples<'a> {
    curve: CatmullRom<'a>,
    span: usize,
    step: u16,
    remaining: usize,
}

impl<'a> CatmullRom<'a> {
    /// The curve through `points` at `steps` samples a span.
    #[inline]
    pub const fn new(points: &'a [Vec2], steps: NonZeroU16) -> Self {
        Self { points, steps }
    }

    /// Returns the number of spans, one fewer than the points, and none for fewer than two.
    #[inline]
    #[must_use]
    pub fn span_count(self) -> usize {
        self.points.len().saturating_sub(1)
    }

    /// Returns the number of samples: `steps` for each span and one more for the last point, the
    /// one point alone for a curve through one point, and none for a curve through none. A count
    /// past `usize::MAX` stops there.
    #[inline]
    #[must_use]
    pub fn sample_count(self) -> usize {
        let empty = self.points.is_empty();
        if empty {
            return 0;
        }
        self.span_count().saturating_mul(usize::from(self.steps.get())).saturating_add(1)
    }

    /// Returns the samples in order: span by span, `steps` evenly in its parameter from its start,
    /// then the last point.
    #[inline]
    pub fn samples(self) -> CatmullRomSamples<'a> {
        CatmullRomSamples { curve: self, span: 0, step: 0, remaining: self.sample_count() }
    }

    /// Returns the point at `amount` through span `span`: its start at 0 and its end at 1. An
    /// amount outside 0 to 1 runs on along the span's cubic. `None` when there is no span `span`.
    #[must_use]
    pub fn point(self, span: usize, amount: f32) -> Option<Vec2> {
        let start = *self.points.get(span)?;
        let end = *self.points.get(span.checked_add(1)?)?;
        let before = span.checked_sub(1).and_then(|index| self.points.get(index));
        let after = span.checked_add(2).and_then(|index| self.points.get(index));
        let before = before.copied().unwrap_or_else(|| reflect(start, end));
        let after = after.copied().unwrap_or_else(|| reflect(end, start));
        Some(centripetal([before, start, end, after], amount))
    }
}

impl Iterator for CatmullRomSamples<'_> {
    type Item = Vec2;

    #[inline]
    fn next(&mut self) -> Option<Vec2> {
        self.remaining = self.remaining.checked_sub(1)?;
        let last = self.remaining == 0;
        if last {
            return self.curve.points.last().copied();
        }
        let steps = self.curve.steps.get();
        let amount = f32::from(self.step) / f32::from(steps);
        let sample = self.curve.point(self.span, amount);
        self.step += 1;
        let span_done = self.step == steps;
        if span_done {
            self.step = 0;
            self.span += 1;
        }
        sample
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for CatmullRomSamples<'_> {}

/// Returns `through` reflected away from `from`: `from` mirrored through `through`.
#[inline]
fn reflect(through: Vec2, from: Vec2) -> Vec2 {
    through + (through - from)
}

/// Returns the knot spacing between `from` and `to`: the square root of their distance.
#[inline]
fn knot_spacing(from: Vec2, to: Vec2) -> f32 {
    libm::sqrtf((to - from).length())
}

/// Returns the point at `amount` through the span from `points[1]` to `points[2]`, with
/// `points[0]` and `points[3]` its neighbours, as a cubic Hermite whose tangents are the
/// centripetal Catmull–Rom tangents scaled to the span's knot spacing. A span of zero length stays
/// at its start, and a neighbour that repeats the span's end is reflected through it instead.
fn centripetal(points: [Vec2; 4], amount: f32) -> Vec2 {
    let [before, start, end, after] = points;
    let middle = knot_spacing(start, end);
    let still = middle <= 0.0;
    if still {
        return start;
    }
    let (before, leading) = neighbour(before, start, end, middle);
    let (after, trailing) = neighbour(after, end, start, middle);
    let start_tangent =
        ((start - before) / leading - (end - before) / (leading + middle) + (end - start) / middle) * middle;
    let end_tangent =
        ((end - start) / middle - (after - start) / (middle + trailing) + (after - end) / trailing) * middle;
    let squared = amount * amount;
    let cubed = squared * amount;
    let start_weight = 2.0 * cubed - 3.0 * squared + 1.0;
    let start_tangent_weight = cubed - 2.0 * squared + amount;
    let end_weight = 3.0 * squared - 2.0 * cubed;
    let end_tangent_weight = cubed - squared;
    start * start_weight + start_tangent * start_tangent_weight + end * end_weight + end_tangent * end_tangent_weight
}

/// Returns the neighbour beyond `near` and its knot spacing: `neighbour` itself when apart from
/// `near`, else `far` reflected through `near`, spaced as the span is, `middle`.
fn neighbour(neighbour: Vec2, near: Vec2, far: Vec2, middle: f32) -> (Vec2, f32) {
    let spacing = knot_spacing(neighbour, near);
    let apart = spacing > 0.0;
    if apart {
        (neighbour, spacing)
    } else {
        (reflect(near, far), middle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close_vec2;

    const WAVE: [Vec2; 5] =
        [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 2.5), Vec2::new(3.5, 0.0), Vec2::new(6.0, -1.0)];

    fn steps(count: u16) -> NonZeroU16 {
        let Some(steps) = NonZeroU16::new(count) else {
            panic!("a test's step count is above zero");
        };
        steps
    }

    // The Barry–Goldman pyramid, written from the knots: an independent evaluation of the same
    // curve, so a slip in the tangent form shows as a disagreement.
    fn barry_goldman(points: [Vec2; 4], amount: f32) -> Vec2 {
        let [p0, p1, p2, p3] = points;
        let t0 = 0.0;
        let t1 = t0 + knot_spacing(p0, p1);
        let t2 = t1 + knot_spacing(p1, p2);
        let t3 = t2 + knot_spacing(p2, p3);
        let t = t1 + (t2 - t1) * amount;
        let blend =
            |a: Vec2, b: Vec2, low: f32, high: f32| a * ((high - t) / (high - low)) + b * ((t - low) / (high - low));
        let a1 = blend(p0, p1, t0, t1);
        let a2 = blend(p1, p2, t1, t2);
        let a3 = blend(p2, p3, t2, t3);
        let b1 = blend(a1, a2, t0, t2);
        let b2 = blend(a2, a3, t1, t3);
        blend(b1, b2, t1, t2)
    }

    // Every step a span lands on a point, in order, and the last sample is the last point.
    #[test]
    fn passes_through_its_points() {
        let curve = CatmullRom::new(&WAVE, steps(7));
        for (index, sample) in curve.samples().enumerate() {
            let on_point = index % 7 == 0;
            if on_point {
                let point = WAVE[index / 7];
                assert!(close_vec2(sample, point, 1e-6), "sample {index}: {sample:?} against {point:?}");
            }
        }
        assert_eq!(curve.samples().len(), 29);
        assert_eq!(curve.samples().count(), 29);
        assert_eq!(curve.samples().last(), Some(WAVE[4]));
    }

    // Inside spans and end spans, against the pyramid with the end's neighbour reflected.
    #[test]
    fn matches_the_pyramid_form() {
        let curve = CatmullRom::new(&WAVE, steps(4));
        let first_neighbours = [reflect(WAVE[0], WAVE[1]), WAVE[0], WAVE[1], WAVE[2]];
        let last_neighbours = [WAVE[2], WAVE[3], WAVE[4], reflect(WAVE[4], WAVE[3])];
        for amount in [0.1, 0.25, 0.5, 0.8] {
            let cases = [
                (1, [WAVE[0], WAVE[1], WAVE[2], WAVE[3]]),
                (2, [WAVE[1], WAVE[2], WAVE[3], WAVE[4]]),
                (0, first_neighbours),
                (3, last_neighbours),
            ];
            for (span, neighbours) in cases {
                let Some(sample) = curve.point(span, amount) else {
                    panic!("span {span} exists");
                };
                let expected = barry_goldman(neighbours, amount);
                let matches = close_vec2(sample, expected, 1e-5);
                assert!(matches, "span {span} at {amount}: {sample:?} against {expected:?}");
            }
        }
    }

    // The first step leaves along the first span and the last arrives along the last, to within
    // the curvature a step of a thousandth of a span can show.
    #[test]
    fn ends_follow_the_end_spans() {
        let curve = CatmullRom::new(&WAVE, steps(1000));
        let Some(leaving) = curve.point(0, 0.001) else {
            panic!("the first span exists");
        };
        let Some(arriving) = curve.point(3, 0.999) else {
            panic!("the last span exists");
        };
        let first_span = (WAVE[1] - WAVE[0]).normalize();
        let last_span = (WAVE[4] - WAVE[3]).normalize();
        let leaving_skew = first_span.perp_dot((leaving - WAVE[0]).normalize());
        let arriving_skew = last_span.perp_dot((WAVE[4] - arriving).normalize());
        assert!(leaving_skew.abs() < 1e-2, "{leaving_skew}");
        assert!(arriving_skew.abs() < 1e-2, "{arriving_skew}");
    }

    // Two points make a straight line, sampled evenly.
    #[test]
    fn two_points_are_a_straight_even_line() {
        let ends = [Vec2::new(1.0, 1.0), Vec2::new(5.0, 3.0)];
        let curve = CatmullRom::new(&ends, steps(4));
        for (index, sample) in (0_u8..).zip(curve.samples()) {
            let expected = ends[0].lerp(ends[1], f32::from(index) / 4.0);
            assert!(close_vec2(sample, expected, 1e-6), "sample {index}: {sample:?} against {expected:?}");
        }
    }

    // A repeated point stays put for a span, and nothing either side of it turns to NaN.
    #[test]
    fn repeated_points_stay_finite() {
        let repeated = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 0.0)];
        let curve = CatmullRom::new(&repeated, steps(5));
        for sample in curve.samples() {
            let finite = sample.x.is_finite() && sample.y.is_finite();
            assert!(finite, "{sample:?}");
        }
        let Some(still) = curve.point(1, 0.5) else {
            panic!("the repeated span exists");
        };
        assert_eq!(still, Vec2::new(1.0, 1.0));
        let Some(all_one) = CatmullRom::new(&[Vec2::ONE; 3], steps(3)).point(0, 0.5) else {
            panic!("the span exists");
        };
        assert_eq!(all_one, Vec2::ONE);
    }

    // One point is its own one sample; none is none; a span past the end is none.
    #[test]
    fn short_curves() {
        let single = [Vec2::new(2.0, 3.0)];
        let one = CatmullRom::new(&single, steps(8));
        let none = CatmullRom::new(&[], steps(8));
        assert_eq!(one.samples().len(), 1);
        assert_eq!(one.samples().next(), Some(single[0]));
        assert_eq!(none.samples().len(), 0);
        assert_eq!(none.samples().next(), None);
        assert_eq!(one.point(0, 0.5), None);
        assert_eq!(CatmullRom::new(&WAVE, steps(2)).point(4, 0.5), None);
    }
}
