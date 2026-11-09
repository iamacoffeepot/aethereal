//! Multi-channel signed distance fields (MSDFs) of glyphs, made as Chlumský's msdfgen makes them: the outline's edges
//! coloured so that the two edges at each corner differ in a channel, then each texel's signed distance to the nearest
//! edge of each channel, exact to lines and quadratic curves and extended past an edge's ends as a pseudo-distance,
//! then each texel's sign checked against the outline, and clashing texels evened out. The median of a texel's three
//! channels is its distance to the outline, with corners kept sharp, so one field at [`BASE_PIXELS_PER_EM`] draws the
//! glyph at any size (ADR-0052).
//!
//! The arithmetic is `f64`, as msdfgen's is: the roots of a cubic are found for every texel and edge.

// A fused multiply-add is a library call on a target without the instruction, as wasm32 and baseline x86-64 are, and
// the distances here are measured for every texel and edge.
#![allow(clippy::suboptimal_flops)]

#[cfg(test)]
mod tests;

mod colour;
mod correct;
mod geometry;
mod sign;

use super::outline::Outline;
use geometry::{Distance, Edge, Nearest, Point};

/// The size the renderer generates every glyph at, in pixels to the em, from which every size is drawn (ADR-0052).
pub const BASE_PIXELS_PER_EM: f32 = 48.0;

/// How far a glyph's field reaches past its outline on every side, in pixels, unless asked otherwise: 4 pixels at the
/// base size is 1 pixel at 12 pixels to the em, room to draw the edge smoothly at every size, and an outline a little
/// wider.
pub const DEFAULT_PADDING: u32 = 4;

/// A glyph's multi-channel signed distance field, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msdf {
    pub width: u32,
    pub height: u32,
    /// Red, green, and blue, a byte each, row by row from the top. Each channel is a signed distance to the outline:
    /// 0.5 (a byte of 127.5) on it, rising inside, and reaching 0 and 1 at [`Msdf::range`] pixels outside and inside.
    pub pixels: Vec<u8>,
    /// How far right of the glyph's origin the field's left edge is, in pixels.
    pub left: i32,
    /// How far above the glyph's baseline the field's top edge is, in pixels.
    pub top: i32,
    /// How far either side of the outline, in pixels, the field reaches from 0.5 to 0 and to 1: its padding.
    pub range: u32,
}

/// The texels of a field: how many, and where its top left is, in pixels from the glyph's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grid {
    left: i32,
    top: i32,
    width: usize,
    height: usize,
}

/// The nearest edge of one channel found so far for a texel.
#[derive(Debug, Clone, Copy)]
struct Channel {
    edge: Edge,
    nearest: Nearest,
}

/// The threshold of msdfgen's error correction: a clash is a difference of more than an edge's worth of distance
/// across one texel.
const CLASH: f64 = 1.001;

/// Generates the field of `outline`, in font units, at `pixels_per_unit` pixels to the font unit, reaching `padding`
/// pixels past the outline on every side: at least one, so that the field has a range. An outline of no contours, as
/// a space's, makes a field of no texels.
#[must_use]
pub fn generate(outline: &Outline, pixels_per_unit: f32, padding: u32) -> Msdf {
    let padding = padding.max(1);
    let contours = contours(outline, pixels_per_unit);
    let Some(grid) = Grid::around(&contours, padding) else {
        return Msdf { width: 0, height: 0, pixels: Vec::new(), left: 0, top: 0, range: padding };
    };

    let edges = colour::colour(contours).into_iter().flatten().collect::<Vec<_>>();
    let mut field = (0..grid.height)
        .flat_map(|row| (0..grid.width).map(move |column| grid.centre(column, row)))
        .map(|centre| distances(&edges, centre))
        .collect::<Vec<_>>();
    sign::correct(&mut field, grid, &edges);

    let span = f64::from(padding) * 2.0;
    for texel in &mut field {
        *texel = texel.map(|distance| distance / span + 0.5);
    }
    correct::correct(&mut field, grid, CLASH / span);

    Msdf {
        width: grid.width_texels(),
        height: grid.height_texels(),
        pixels: field.into_iter().flatten().map(byte).collect(),
        left: grid.left,
        top: grid.top,
        range: padding,
    }
}

impl Msdf {
    /// The median of the field's three channels at `x` and `y`, in texels from the field's top left, sampled between
    /// texels' centres as the glyph program's filtering samples them: above 0.5 inside the glyph, below outside.
    #[must_use]
    pub fn median(&self, x: f32, y: f32) -> f32 {
        if self.pixels.is_empty() {
            return 0.0;
        }
        let texel = |column: i64, row: i64| -> [f32; 3] {
            let column = column.clamp(0, i64::from(self.width) - 1);
            let row = row.clamp(0, i64::from(self.height) - 1);
            let at = usize::try_from((row * i64::from(self.width) + column) * 3).unwrap_or(0);
            let channel = |offset: usize| f32::from(self.pixels.get(at + offset).copied().unwrap_or(0)) / 255.0;
            [channel(0), channel(1), channel(2)]
        };

        let (x, y) = (x - 0.5, y - 0.5);
        let (column, row) = (x.floor(), y.floor());
        let (across, down) = (x - column, y - row);
        #[allow(clippy::cast_possible_truncation, reason = "a texel's index is far inside i64")]
        let (column, row) = (column as i64, row as i64);
        let lerp = |a: [f32; 3], b: [f32; 3], amount: f32| {
            [0, 1, 2].map(|channel| a[channel] + (b[channel] - a[channel]) * amount)
        };
        let above = lerp(texel(column, row), texel(column + 1, row), across);
        let below = lerp(texel(column, row + 1), texel(column + 1, row + 1), across);
        let [red, green, blue] = lerp(above, below, down);
        red.min(green).max(red.max(green).min(blue))
    }
}

impl Grid {
    /// The grid of whole pixels around `contours`' bounds, `padding` pixels past them on every side, if there are any.
    #[allow(clippy::cast_possible_truncation, reason = "a glyph's bounds in pixels are far inside i32")]
    fn around(contours: &[Vec<Edge>], padding: u32) -> Option<Self> {
        let mut points = contours.iter().flatten().flat_map(|&edge| corners(edge));
        let first = points.next()?;
        let (low, high) = points.fold((first, first), |(low, high), point| {
            (Point::new(low.x.min(point.x), low.y.min(point.y)), Point::new(high.x.max(point.x), high.y.max(point.y)))
        });

        let padding = i64::from(padding);
        let left = low.x.floor() as i64 - padding;
        let right = high.x.ceil() as i64 + padding;
        let bottom = low.y.floor() as i64 - padding;
        let top = high.y.ceil() as i64 + padding;
        Some(Self {
            left: i32::try_from(left).ok()?,
            top: i32::try_from(top).ok()?,
            width: usize::try_from(right - left).ok()?,
            height: usize::try_from(top - bottom).ok()?,
        })
    }

    /// The centre of the texel at `column` from the left and `row` from the top, in pixels from the glyph's origin.
    #[allow(clippy::cast_precision_loss, reason = "a texel's column and row are far inside f64's exact integers")]
    fn centre(self, column: usize, row: usize) -> Point {
        Point::new(f64::from(self.left) + column as f64 + 0.5, f64::from(self.top) - row as f64 - 0.5)
    }

    /// The index of the texel `across` columns and `down` rows from the one at `column` and `row`, if it is in the
    /// grid.
    fn neighbour(self, column: usize, row: usize, across: isize, down: isize) -> Option<usize> {
        let column = column.checked_add_signed(across).filter(|&column| column < self.width)?;
        let row = row.checked_add_signed(down).filter(|&row| row < self.height)?;
        Some(row * self.width + column)
    }

    /// How many texels wide the grid is.
    fn width_texels(self) -> u32 {
        u32::try_from(self.width).unwrap_or(u32::MAX)
    }

    /// How many texels tall the grid is.
    fn height_texels(self) -> u32 {
        u32::try_from(self.height).unwrap_or(u32::MAX)
    }
}

/// The contours of `outline` as edges in pixels, at `pixels_per_unit` pixels to the font unit, those of no length left
/// out, and contours left with no edges.
fn contours(outline: &Outline, pixels_per_unit: f32) -> Vec<Vec<Edge>> {
    let scale = f64::from(pixels_per_unit);
    outline
        .contours
        .iter()
        .map(|contour| {
            contour
                .segments
                .iter()
                .map(|&segment| Edge::from_segment(segment, scale))
                .filter(|edge| !edge.is_degenerate())
                .collect::<Vec<_>>()
        })
        .filter(|edges| !edges.is_empty())
        .collect()
}

/// The points that bound `edge`: its ends, and a curve's turning points.
fn corners(edge: Edge) -> impl Iterator<Item = Point> {
    let turns = match edge {
        Edge::Line(_) => [0.0, 0.0],
        Edge::Quadratic([start, control, end]) => {
            let turn = |start: f64, control: f64, end: f64| {
                let bend = start - 2.0 * control + end;
                let along = (start - control) / bend;
                let inside = bend != 0.0 && along > 0.0 && along < 1.0;
                if inside {
                    along
                } else {
                    0.0
                }
            };
            [turn(start.x, control.x, end.x), turn(start.y, control.y, end.y)]
        }
    };
    [edge.start(), edge.end(), edge.point(turns[0]), edge.point(turns[1])].into_iter()
}

/// The signed pseudo-distance from `centre` to the nearest edge of each channel, in pixels, positive inside.
fn distances(edges: &[colour::Coloured], centre: Point) -> [f64; 3] {
    let mut nearest = Channel { edge: edges[0].edge, nearest: Nearest { distance: Distance::FAR, along: 0.0 } };
    let mut channels = [nearest; 3];
    let mut found = [false; 3];
    for coloured in edges {
        let candidate = Channel { edge: coloured.edge, nearest: coloured.edge.nearest(centre) };
        if candidate.nearest.distance.nearer(nearest.nearest.distance) {
            nearest = candidate;
        }
        for (channel, best) in channels.iter_mut().enumerate() {
            let measured = coloured.colour.has(channel);
            let nearer = candidate.nearest.distance.nearer(best.nearest.distance);
            if measured && nearer {
                *best = candidate;
                found[channel] = true;
            }
        }
    }

    [0, 1, 2].map(|channel| {
        let best = if found[channel] {
            channels[channel]
        } else {
            nearest
        };
        best.edge.pseudo_distance(best.nearest, centre)
    })
}

/// A field's value, from 0 to 1, as a byte.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped from 0 to 255 before")]
fn byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
