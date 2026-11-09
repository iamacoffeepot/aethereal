//! The edges a field is measured from, in the field's pixels and in `f64`, and the signed distance from a point to each:
//! exact to a line, and to a quadratic curve by the roots of a cubic, as msdfgen measures them.

use crate::font::outline::Segment;
use std::f64::consts::PI;
use std::ops::{Add, Mul, Neg, Sub};

/// A point, or a vector, in the field's pixels, `y` up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// An edge of a contour, in the field's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    /// A straight line from the first point to the second.
    Line([Point; 2]),
    /// A quadratic curve from the first point to the third, the second its control point.
    Quadratic([Point; 3]),
}

/// A signed distance from a point to an edge, positive on the edge's right, which is a TrueType contour's inside, and
/// how squarely the point faces the edge's end when the nearest point is that end: `dot` is 0 when the nearest point is
/// inside the edge, and the cosine between the edge's direction and the way to the point when it is an end, so of two
/// edges meeting at a corner, the one the point is more square to is nearer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    pub distance: f64,
    pub dot: f64,
}

/// The distance from a point to an edge, and where along the edge, from 0 at its start to 1 at its end, the nearest
/// point is: below 0 or past 1 when the nearest point is an end and the point lies beyond it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    pub distance: Distance,
    pub along: f64,
}

impl Point {
    /// The point at `x` and `y`.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// The 2D cross product: positive when `other` turns anticlockwise from `self`.
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// The length.
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// The vector of length one along this one: `y` for the zero vector, as msdfgen takes it.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length == 0.0 {
            return Self::new(0.0, 1.0);
        }
        self * (1.0 / length)
    }

    /// The point `amount` of the way from `self` to `other`.
    pub fn lerp(self, other: Self, amount: f64) -> Self {
        self + (other - self) * amount
    }
}

impl Add for Point {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, scale: f64) -> Self {
        Self::new(self.x * scale, self.y * scale)
    }
}

impl Neg for Point {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl Distance {
    /// Further than any edge.
    pub const FAR: Self = Self { distance: f64::MAX, dot: 1.0 };

    /// Whether this distance is nearer than `other`: shorter, or as short and more square to its edge.
    pub fn nearer(self, other: Self) -> bool {
        let shorter = self.distance.abs() < other.distance.abs();
        let as_short = self.distance.abs() == other.distance.abs();
        let squarer = self.dot < other.dot;
        shorter || (as_short && squarer)
    }
}

impl Edge {
    /// `segment`, in font units, scaled by `scale` into the field's pixels.
    pub fn from_segment(segment: Segment, scale: f64) -> Self {
        let point = |at: aethereal_math::Vec2| Point::new(f64::from(at.x) * scale, f64::from(at.y) * scale);
        match segment {
            Segment::Line { from, to } => Self::Line([point(from), point(to)]),
            Segment::Quadratic { from, control, to } => Self::Quadratic([point(from), point(control), point(to)]),
        }
    }

    /// Where the edge starts.
    pub const fn start(self) -> Point {
        match self {
            Self::Line([start, _]) | Self::Quadratic([start, _, _]) => start,
        }
    }

    /// Where the edge ends.
    pub const fn end(self) -> Point {
        match self {
            Self::Line([_, end]) | Self::Quadratic([_, _, end]) => end,
        }
    }

    /// Whether the edge has no length: every point of it the same.
    pub fn is_degenerate(self) -> bool {
        match self {
            Self::Line([start, end]) => start == end,
            Self::Quadratic([start, control, end]) => start == control && control == end,
        }
    }

    /// The point `along` the edge, from 0 at its start to 1 at its end.
    pub fn point(self, along: f64) -> Point {
        match self {
            Self::Line([start, end]) => start.lerp(end, along),
            Self::Quadratic([start, control, end]) => start.lerp(control, along).lerp(control.lerp(end, along), along),
        }
    }

    /// The edge's direction `along` it, not normalized: a curve whose control point sits on an end takes the way from
    /// its start to its end there.
    pub fn direction(self, along: f64) -> Point {
        match self {
            Self::Line([start, end]) => end - start,
            Self::Quadratic([start, control, end]) => {
                let tangent = (control - start).lerp(end - control, along);
                let still = tangent.x == 0.0 && tangent.y == 0.0;
                if still {
                    end - start
                } else {
                    tangent
                }
            }
        }
    }

    /// The edge cut into thirds, as msdfgen cuts one when a contour has too few edges for three colours.
    pub fn thirds(self) -> [Self; 3] {
        match self {
            Self::Line([start, end]) => {
                let (first, second) = (start.lerp(end, 1.0 / 3.0), start.lerp(end, 2.0 / 3.0));
                [Self::Line([start, first]), Self::Line([first, second]), Self::Line([second, end])]
            }
            Self::Quadratic([start, control, end]) => {
                let (first, second) = (self.point(1.0 / 3.0), self.point(2.0 / 3.0));
                let middle = start.lerp(control, 5.0 / 9.0).lerp(control.lerp(end, 4.0 / 9.0), 0.5);
                [
                    Self::Quadratic([start, start.lerp(control, 1.0 / 3.0), first]),
                    Self::Quadratic([first, middle, second]),
                    Self::Quadratic([second, control.lerp(end, 2.0 / 3.0), end]),
                ]
            }
        }
    }

    /// The signed distance from `origin` to the edge's nearest point.
    pub fn nearest(self, origin: Point) -> Nearest {
        match self {
            Self::Line([start, end]) => line_nearest(start, end, origin),
            Self::Quadratic([start, control, end]) => self.quadratic_nearest(start, control, end, origin),
        }
    }

    /// The distance from `origin` to the edge's line extended past the end nearest it, when that is nearer than the
    /// true distance and `origin` lies beyond that end: msdfgen's pseudo-distance, which keeps two edges meeting at a
    /// corner from rounding it.
    pub fn pseudo_distance(self, nearest: Nearest, origin: Point) -> f64 {
        let beyond = |at: Point, direction: Point, ahead: bool| {
            let direction = direction.normalized();
            let away = origin - at;
            let along = away.dot(direction);
            let past = if ahead {
                along > 0.0
            } else {
                along < 0.0
            };
            let pseudo = away.cross(direction);
            let shorter = pseudo.abs() <= nearest.distance.distance.abs();
            if past && shorter {
                pseudo
            } else {
                nearest.distance.distance
            }
        };
        if nearest.along < 0.0 {
            return beyond(self.start(), self.direction(0.0), false);
        }
        if nearest.along > 1.0 {
            return beyond(self.end(), self.direction(1.0), true);
        }
        nearest.distance.distance
    }

    /// The signed distance from `origin` to a quadratic curve: the nearest of its ends and of the points where the
    /// way from `origin` is square to the curve, which are the roots of a cubic.
    fn quadratic_nearest(self, start: Point, control: Point, end: Point, origin: Point) -> Nearest {
        let to_start = start - origin;
        let first = control - start;
        let bend = end - control - first;
        let roots = cubic(
            bend.dot(bend),
            3.0 * first.dot(bend),
            2.0 * first.dot(first) + to_start.dot(bend),
            to_start.dot(first),
        );

        let leaving = self.direction(0.0);
        let mut distance = sign(leaving.cross(to_start)) * to_start.length();
        let mut along = -to_start.dot(leaving) / leaving.dot(leaving);

        let arriving = self.direction(1.0);
        let to_end = end - origin;
        let end_distance = to_end.length();
        if end_distance < distance.abs() {
            distance = sign(arriving.cross(to_end)) * end_distance;
            along = (origin - control).dot(arriving) / arriving.dot(arriving);
        }

        for root in roots.iter().filter(|root| *root > 0.0 && *root < 1.0) {
            let to_point = to_start + first * (2.0 * root) + bend * (root * root);
            let root_distance = to_point.length();
            if root_distance <= distance.abs() {
                distance = sign((first + bend * root).cross(to_point)) * root_distance;
                along = root;
            }
        }

        let dot = if (0.0..=1.0).contains(&along) {
            0.0
        } else if along < 0.5 {
            leaving.normalized().dot(to_start.normalized()).abs()
        } else {
            arriving.normalized().dot(to_end.normalized()).abs()
        };
        Nearest { distance: Distance { distance, dot }, along }
    }
}

/// The signed distance from `origin` to the line from `start` to `end`.
fn line_nearest(start: Point, end: Point, origin: Point) -> Nearest {
    let from_start = origin - start;
    let line = end - start;
    let along = from_start.dot(line) / line.dot(line);
    let nearer_end = if along > 0.5 {
        end
    } else {
        start
    };
    let to_end = nearer_end - origin;
    let end_distance = to_end.length();

    let inside = along > 0.0 && along < 1.0;
    if inside {
        let square = from_start.cross(line) / line.length();
        if square.abs() < end_distance {
            return Nearest { distance: Distance { distance: square, dot: 0.0 }, along };
        }
    }

    let distance = sign(from_start.cross(line)) * end_distance;
    let dot = line.normalized().dot(to_end.normalized()).abs();
    Nearest { distance: Distance { distance, dot }, along }
}

/// The sign of `value`, positive for zero, as msdfgen's `nonZeroSign`.
fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Up to three real roots of an equation, kept without allocating, as one is solved for every texel and edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roots {
    values: [f64; 3],
    count: usize,
}

impl Roots {
    /// No roots.
    const NONE: Self = Self { values: [0.0; 3], count: 0 };

    /// The roots `values`.
    fn of<const N: usize>(values: [f64; N]) -> Self {
        let mut roots = Self::NONE;
        roots.values[..N].copy_from_slice(&values);
        roots.count = N;
        roots
    }

    /// Each root.
    pub fn iter(self) -> impl Iterator<Item = f64> {
        self.values.into_iter().take(self.count)
    }
}

/// The real roots of `a x³ + b x² + c x + d`, as msdfgen solves it: a cubic whose `a` is tiny beside `b` is solved as
/// the quadratic it nearly is.
pub fn cubic(a: f64, b: f64, c: f64, d: f64) -> Roots {
    let normed = b / a;
    let cubic = a != 0.0 && normed.abs() < 1e6;
    if cubic {
        return normed_cubic(normed, c / a, d / a);
    }
    quadratic(b, c, d)
}

/// The real roots of `x³ + a2 x² + a1 x + a0`, by Cardano's and Viète's formulas.
fn normed_cubic(a2: f64, a1: f64, a0: f64) -> Roots {
    let a2_squared = a2 * a2;
    let q = (a2_squared - 3.0 * a1) / 9.0;
    let r = (a2 * (2.0 * a2_squared - 9.0 * a1) + 27.0 * a0) / 54.0;
    let r_squared = r * r;
    let q_cubed = q * q * q;
    let shift = a2 / 3.0;
    if r_squared < q_cubed {
        let angle = (r / q_cubed.sqrt()).clamp(-1.0, 1.0).acos();
        let scale = -2.0 * q.sqrt();
        return Roots::of([
            scale * (angle / 3.0).cos() - shift,
            scale * ((angle + 2.0 * PI) / 3.0).cos() - shift,
            scale * ((angle - 2.0 * PI) / 3.0).cos() - shift,
        ]);
    }

    let magnitude = (r.abs() + (r_squared - q_cubed).sqrt()).cbrt();
    let u = if r < 0.0 {
        magnitude
    } else {
        -magnitude
    };
    let v = if u == 0.0 {
        0.0
    } else {
        q / u
    };
    let one = (u + v) - shift;
    let double = u == v || (u - v).abs() < 1e-12 * (u + v).abs();
    if double {
        return Roots::of([one, -0.5 * (u + v) - shift]);
    }
    Roots::of([one])
}

/// The real roots of `a x² + b x + c`: none for an equation true of every `x`, or of none.
fn quadratic(a: f64, b: f64, c: f64) -> Roots {
    let linear = a == 0.0 || b.abs() > 1e12 * a.abs();
    if linear {
        if b == 0.0 {
            return Roots::NONE;
        }
        return Roots::of([-c / b]);
    }

    let discriminant = b * b - 4.0 * a * c;
    if discriminant > 0.0 {
        let root = discriminant.sqrt();
        return Roots::of([(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)]);
    }
    if discriminant == 0.0 {
        return Roots::of([-b / (2.0 * a)]);
    }
    Roots::NONE
}
