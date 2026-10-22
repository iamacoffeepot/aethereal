use crate::{Polyline, Segment, Vec2};

impl Polyline<'_> {
    /// Returns the tightest radius the line bends at.
    ///
    /// At each point where one step meets the next, the radius is the mean length of the two steps
    /// over the angle the line turns there, in radians: the radius of the circle whose arc of that
    /// length turns by that angle. Along points sampled evenly from a circle it approaches the
    /// circle's radius as the samples get closer. A closed polyline bends at every point,
    /// including where its closing segment meets its first; an open one only between its ends.
    ///
    /// A step of zero length has no direction, so repeated points are passed over and the steps on
    /// either side of them meet. A line that never turns, or has fewer than two steps of positive
    /// length, answers `f32::INFINITY`. A line that turns straight back answers the mean step
    /// length over `π`.
    #[must_use]
    pub fn tightest_bend_radius(self) -> f32 {
        let mut steps = self.segments().map(Segment::direction).filter(|step| step.length_squared() > 0.0);
        let Some(first) = steps.next() else {
            return f32::INFINITY;
        };
        let (last, open_tightest) = steps.fold((first, f32::INFINITY), |(incoming, tightest), outgoing| {
            (outgoing, tightest.min(bend_radius(incoming, outgoing)))
        });
        let loops = self.loops();
        if loops {
            open_tightest.min(bend_radius(last, first))
        } else {
            open_tightest
        }
    }
}

/// Returns the radius of the bend from `incoming` to `outgoing`, both of positive length:
/// infinite where they point the same way.
fn bend_radius(incoming: Vec2, outgoing: Vec2) -> f32 {
    let turn = libm::atan2f(incoming.perp_dot(outgoing).abs(), incoming.dot(outgoing));
    let straight = turn <= 0.0;
    if straight {
        return f32::INFINITY;
    }
    let mean_step = (incoming.length() + outgoing.length()) * 0.5;
    mean_step / turn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close;
    use core::f32::consts::{FRAC_PI_2, PI, SQRT_2, TAU};

    // Even samples of a circle bend at the circle's radius, shortened by the chord: the step is
    // `2 r sin(π / n)` and each point turns by `2π / n`.
    #[test]
    fn circle_samples_bend_at_its_radius() {
        let radius = 3.0;
        let count: u16 = 48;
        let mut points = [Vec2::ZERO; 48];
        for (index, point) in (0..count).zip(points.iter_mut()) {
            let angle = TAU * f32::from(index) / f32::from(count);
            *point = Vec2::new(libm::cosf(angle), libm::sinf(angle)) * radius;
        }
        let half_turn = PI / f32::from(count);
        let expected = radius * libm::sinf(half_turn) / half_turn;
        let tightest = Polyline::closed(&points).tightest_bend_radius();
        let matches = close(tightest, expected, 1e-4);
        assert!(matches, "{tightest} against {expected}");
    }

    // Open, a right angle with arms of 10 bends once by a quarter turn. Closed, the diagonal adds
    // two bends of three eighths of a turn, between steps of 10 and `10 √2`, which are tighter.
    #[test]
    fn closed_line_bends_where_it_closes() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        let open = Polyline::open(&points).tightest_bend_radius();
        let closed = Polyline::closed(&points).tightest_bend_radius();
        let open_expected = 10.0 / FRAC_PI_2;
        let closed_expected = (5.0 + 5.0 * SQRT_2) / (0.75 * PI);
        let open_matches = close(open, open_expected, 1e-5);
        let closed_matches = close(closed, closed_expected, 1e-5);
        assert!(open_matches, "{open} against {open_expected}");
        assert!(closed_matches, "{closed} against {closed_expected}");
    }

    // A repeated point at a corner is passed over: the corner still turns a quarter.
    #[test]
    fn repeated_points_are_passed_over() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)];
        let tightest = Polyline::open(&points).tightest_bend_radius();
        let matches = close(tightest, 1.0 / FRAC_PI_2, 1e-6);
        assert!(matches, "{tightest}");
    }

    // A line that turns straight back turns by a half turn.
    #[test]
    fn reversal_turns_by_pi() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 0.0)];
        let tightest = Polyline::open(&points).tightest_bend_radius();
        let matches = close(tightest, 1.0 / PI, 1e-6);
        assert!(matches, "{tightest}");
    }

    // With nothing to turn, no radius is tight.
    #[test]
    fn straight_and_short_lines_never_bend() {
        let straight = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(3.0, 0.0)];
        let repeated = [Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0)];
        assert_eq!(Polyline::open(&straight).tightest_bend_radius(), f32::INFINITY);
        assert_eq!(Polyline::open(&straight[..2]).tightest_bend_radius(), f32::INFINITY);
        assert_eq!(Polyline::closed(&repeated).tightest_bend_radius(), f32::INFINITY);
        assert_eq!(Polyline::open(&[]).tightest_bend_radius(), f32::INFINITY);
    }
}
