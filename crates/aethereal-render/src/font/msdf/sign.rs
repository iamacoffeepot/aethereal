//! Sign correction, as msdfgen's `distanceSignCorrection`: each texel's centre is tested against the outline by the
//! nonzero rule, and a texel whose median distance says otherwise has its three channels' signs turned. So a field is
//! inside exactly where the glyph is at every texel's centre, whatever the contours' winding or overlaps.

use super::Grid;
use super::colour::Coloured;
use super::geometry::{Edge, Point};

/// Where a contour crosses a row, and which way: `1` going up, `-1` going down.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Crossing {
    x: f64,
    winding: i32,
}

/// How a texel's sign compared with the outline's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    /// Its median agreed.
    Kept,
    /// Its median disagreed, and was turned.
    Turned,
    /// Its median was exactly on the edge, and is decided by its neighbours.
    Undecided,
}

/// Turns the sign of each texel of `field` whose median disagrees with whether its centre is inside `edges`.
pub fn correct(field: &mut [[f64; 3]], grid: Grid, edges: &[Coloured]) {
    let mut signs = vec![Sign::Kept; field.len()];
    let mut undecided = false;
    for row in 0..grid.height {
        let crossings = crossings(edges.iter().map(|coloured| coloured.edge), grid.centre(0, row).y);
        for column in 0..grid.width {
            let index = row * grid.width + column;
            let inside = filled(&crossings, grid.centre(column, row).x);
            let median = median(field[index]);
            signs[index] = if median == 0.0 {
                undecided = true;
                Sign::Undecided
            } else if (median > 0.0) == inside {
                Sign::Kept
            } else {
                field[index] = field[index].map(|distance| -distance);
                Sign::Turned
            };
        }
    }

    // A texel on the edge takes its neighbours' side, so a field turned whole leaves no texel unturned in it.
    if !undecided {
        return;
    }
    for row in 0..grid.height {
        for column in 0..grid.width {
            let index = row * grid.width + column;
            if signs[index] != Sign::Undecided {
                continue;
            }
            let neighbours = [
                column.checked_sub(1).map(|left| row * grid.width + left),
                (column + 1 < grid.width).then_some(index + 1),
                row.checked_sub(1).map(|above| above * grid.width + column),
                (row + 1 < grid.height).then_some(index + grid.width),
            ];
            let vote: i32 = neighbours
                .into_iter()
                .flatten()
                .map(|neighbour| match signs[neighbour] {
                    Sign::Kept => 1,
                    Sign::Turned => -1,
                    Sign::Undecided => 0,
                })
                .sum();
            if vote < 0 {
                field[index] = field[index].map(|distance| -distance);
            }
        }
    }
}

/// Whether `at` is inside `edges`, by the nonzero rule.
#[cfg(test)]
pub fn inside(edges: &[Coloured], at: Point) -> bool {
    filled(&crossings(edges.iter().map(|coloured| coloured.edge), at.y), at.x)
}

/// The median of three values.
pub fn median([a, b, c]: [f64; 3]) -> f64 {
    a.min(b).max(a.max(b).min(c))
}

/// Whether a point at `x` on a row is inside, by the nonzero rule: the windings of the crossings to its right sum to
/// other than zero.
fn filled(crossings: &[Crossing], x: f64) -> bool {
    crossings.iter().filter(|crossing| crossing.x > x).map(|crossing| crossing.winding).sum::<i32>() != 0
}

/// Where `edges` cross the row at `y`. Each edge counts its lower end and not its upper, so a row through the point
/// where two edges meet counts it once, and a row touching a curve where it turns counts nothing.
fn crossings(edges: impl Iterator<Item = Edge>, y: f64) -> Vec<Crossing> {
    let mut crossings = Vec::new();
    for edge in edges {
        let [start, control, end] = match edge {
            Edge::Line([start, end]) => [start, start.lerp(end, 0.5), end],
            Edge::Quadratic(points) => points,
        };
        let bend = start.y - 2.0 * control.y + end.y;
        let turn = (start.y - control.y) / bend;
        let turns = bend != 0.0 && turn > 0.0 && turn < 1.0;
        if turns {
            let (first, second) = (start.lerp(control, turn), control.lerp(end, turn));
            let middle = first.lerp(second, turn);
            crossings.extend(cross([start, first, middle], y));
            crossings.extend(cross([middle, second, end], y));
        } else {
            crossings.extend(cross([start, control, end], y));
        }
    }
    crossings
}

/// Where a quadratic curve that only rises or only falls crosses the row at `y`, if it does.
fn cross([start, control, end]: [Point; 3], y: f64) -> Option<Crossing> {
    let rising = start.y <= y && y < end.y;
    let falling = end.y <= y && y < start.y;
    let crosses = rising || falling;
    if !crosses {
        return None;
    }

    let bend = start.y - 2.0 * control.y + end.y;
    let slope = 2.0 * (control.y - start.y);
    let offset = start.y - y;
    let along = if bend.abs() < 1e-12 {
        -offset / slope
    } else {
        let root = (slope * slope - 4.0 * bend * offset).max(0.0).sqrt();
        let first = (-slope + root) / (2.0 * bend);
        let second = (-slope - root) / (2.0 * bend);
        let outside = |along: f64| (along.clamp(0.0, 1.0) - along).abs();
        if outside(first) <= outside(second) {
            first
        } else {
            second
        }
    };

    let along = along.clamp(0.0, 1.0);
    let x = start.lerp(control, along).lerp(control.lerp(end, along), along).x;
    let winding = if rising {
        1
    } else {
        -1
    };
    Some(Crossing { x, winding })
}
