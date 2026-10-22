// The straight piece of a line from `start` to `end`, with the meanings of `aethereal_math::Segment` (ADR-0005): `end`
// equals `start` for a segment of zero length, which is measured as its point and never produces NaN.
public struct Segment {
    start: vec2<f32>,
    end: vec2<f32>,
}

// The side of a segment's line a point lies on, as `side` answers it: the sign of `perp_dot(direction, point - start)`.
// WGSL has no enums, so `Side` is these three `i32` constants, each the sign it names.

// A quarter turn counter-clockwise from the direction the segment runs.
public const SIDE_POSITIVE: i32 = 1;
// A quarter turn clockwise from the direction the segment runs.
public const SIDE_NEGATIVE: i32 = -1;
// On the segment's line, or anywhere for a segment of zero length, which has no line.
public const SIDE_ON: i32 = 0;

// `end - start`.
public fn direction(segment: Segment) -> vec2<f32> {
    return segment.end - segment.start;
}

// The length of the segment. It is not named `length`, which would hide WGSL's built-in.
public fn length_of(segment: Segment) -> f32 {
    return length(direction(segment));
}

// How far along the segment's whole line, from `start` toward `end`, the foot of `point` lies: the foot is not clipped
// to the segment, so the answer is negative before `start` and above `length_of` past `end`. A segment of zero length
// has no line, and answers 0.
public fn along(segment: Segment, point: vec2<f32>) -> f32 {
    let size = length_of(segment);
    if size > 0.0 {
        return dot(direction(segment), point - segment.start) / size;
    }
    return 0.0;
}

// The point of the segment nearest `point`. A segment of zero length answers `start`.
public fn closest_point(segment: Segment, point: vec2<f32>) -> vec2<f32> {
    let travel = direction(segment);
    let span = dot(travel, travel);
    if span > 0.0 {
        let fraction = clamp(dot(travel, point - segment.start) / span, 0.0, 1.0);
        return segment.start + travel * fraction;
    }
    return segment.start;
}

// The distance from `point` to the nearest point of the segment.
public fn distance_to(segment: Segment, point: vec2<f32>) -> f32 {
    return length(point - closest_point(segment, point));
}

// True when `point` lies exactly on the segment, either end included. The test has no tolerance.
public fn contains_point(segment: Segment, point: vec2<f32>) -> bool {
    let on_line = side(segment, point) == SIDE_ON;
    let low = min(segment.start, segment.end);
    let high = max(segment.start, segment.end);
    let above_low = all(point >= low);
    let below_high = all(point <= high);
    return on_line && above_low && below_high;
}

// The side of the segment's line `point` lies on: `SIDE_POSITIVE`, `SIDE_NEGATIVE`, or `SIDE_ON`. A segment of zero
// length answers `SIDE_ON` for every point.
public fn side(segment: Segment, point: vec2<f32>) -> i32 {
    let turn = perp_dot(direction(segment), point - segment.start);
    if turn > 0.0 {
        return SIDE_POSITIVE;
    }
    if turn < 0.0 {
        return SIDE_NEGATIVE;
    }
    return SIDE_ON;
}

// True when `segment` and `other` cross: each passes strictly from one side of the other's line to the other side, or
// they lie on one line and share a stretch of positive length. Segments that only touch do not cross: a shared end, or
// an end of one lying on the other, is a touch. A segment of zero length is a point and crosses nothing. The sides are
// compared exactly, with no tolerance.
public fn crosses(segment: Segment, other: Segment) -> bool {
    let own = direction(segment);
    let theirs = direction(other);
    let their_start_turn = perp_dot(own, other.start - segment.start);
    let their_end_turn = perp_dot(own, other.end - segment.start);
    let own_start_turn = perp_dot(theirs, segment.start - other.start);
    let own_end_turn = perp_dot(theirs, segment.end - other.start);
    let they_straddle = opposite(their_start_turn, their_end_turn);
    let own_straddle = opposite(own_start_turn, own_end_turn);
    if they_straddle && own_straddle {
        return true;
    }
    let their_start_on = their_start_turn == 0.0;
    let their_end_on = their_end_turn == 0.0;
    let has_line = dot(own, own) > 0.0;
    let collinear = their_start_on && their_end_on && has_line;
    return collinear && overlaps_on_line(segment, other);
}

// True when `other`, lying on the line of `segment`, shares a stretch of positive length with it. `segment` has
// positive length.
fn overlaps_on_line(segment: Segment, other: Segment) -> bool {
    let own = direction(segment);
    let span = dot(own, own);
    let first = dot(own, other.start - segment.start) / span;
    let second = dot(own, other.end - segment.start) / span;
    let low = max(min(first, second), 0.0);
    let high = min(max(first, second), 1.0);
    return high > low;
}

// `a.x * b.y - a.y * b.x`: positive when `b` is counter-clockwise of `a`.
fn perp_dot(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

// True when one of `first` and `second` is above zero and the other below.
fn opposite(first: f32, second: f32) -> bool {
    let rising = first < 0.0 && second > 0.0;
    let falling = first > 0.0 && second < 0.0;
    return rising || falling;
}
