//! Edge colouring, as msdfgen's `edgeColoringSimple` does it: each edge is given two of the three channels, and the two
//! edges that meet at a corner share only one, so that each channel's field rounds the corner differently and their
//! median keeps it sharp. A contour with no corner is white, in every channel.

use super::geometry::{Edge, Point};

/// The channels an edge is measured in: red, green, and blue, a bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour(u8);

impl Colour {
    /// No channel.
    pub const BLACK: Self = Self(0);
    /// Green and blue.
    pub const CYAN: Self = Self(6);
    /// Red and blue.
    pub const MAGENTA: Self = Self(5);
    /// Red and green.
    pub const YELLOW: Self = Self(3);
    /// Every channel.
    pub const WHITE: Self = Self(7);

    /// Whether the colour has channel `channel`: 0 red, 1 green, 2 blue.
    pub const fn has(self, channel: usize) -> bool {
        self.0 & (1 << channel) != 0
    }

    /// The channels both colours have.
    pub const fn and(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Whether the colour is one channel alone.
    const fn single(self) -> bool {
        self.0.is_power_of_two()
    }
}

/// An edge and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coloured {
    pub edge: Edge,
    pub colour: Colour,
}

/// The sine of the angle under which two edges meeting turn too little to be a corner: msdfgen's default of 3 radians.
const CORNER: f64 = 0.141_120_008_059_867_2;

/// Colours each contour's edges. A contour of one corner and fewer than three edges has its edges cut into thirds
/// first, so that it has three colours to give.
pub fn colour(contours: Vec<Vec<Edge>>) -> Vec<Vec<Coloured>> {
    let mut seed = 0;
    contours.into_iter().map(|edges| contour(edges, &mut seed)).collect()
}

/// Colours one contour's edges.
fn contour(edges: Vec<Edge>, seed: &mut u64) -> Vec<Coloured> {
    let count = edges.len();
    let corners = (0..count)
        .filter(|&index| {
            let before = edges[(index + count - 1) % count].direction(1.0).normalized();
            let after = edges[index].direction(0.0).normalized();
            is_corner(before, after)
        })
        .collect::<Vec<_>>();

    match corners.as_slice() {
        [] => edges.into_iter().map(|edge| Coloured { edge, colour: Colour::WHITE }).collect(),
        &[corner] => teardrop(&edges, corner, seed),
        _ => between_corners(&edges, &corners, seed),
    }
}

/// Colours a contour of one corner: a third of it in one colour, a third white, and a third in another, from the
/// corner round.
fn teardrop(edges: &[Edge], corner: usize, seed: &mut u64) -> Vec<Coloured> {
    let mut first = Colour::WHITE;
    switch(&mut first, seed, Colour::BLACK);
    let mut last = first;
    switch(&mut last, seed, Colour::BLACK);
    let colours = [first, Colour::WHITE, last];

    let count = edges.len();
    if count >= 3 {
        return (0..count)
            .map(|index| {
                let edge = edges[(corner + index) % count];
                Coloured { edge, colour: colours[trichotomy(index, count)] }
            })
            .collect();
    }

    let thirds = (0..count).flat_map(|index| edges[(corner + index) % count].thirds()).collect::<Vec<_>>();
    let per_colour = thirds.len() / 3;
    thirds.into_iter().enumerate().map(|(index, edge)| Coloured { edge, colour: colours[index / per_colour] }).collect()
}

/// Colours a contour of several corners: the colour switches at each corner, and the last stretch differs from the
/// first, which it meets.
fn between_corners(edges: &[Edge], corners: &[usize], seed: &mut u64) -> Vec<Coloured> {
    let count = edges.len();
    let start = corners[0];
    let mut colour = Colour::WHITE;
    switch(&mut colour, seed, Colour::BLACK);
    let initial = colour;

    let mut spline = 0;
    let mut coloured = vec![Coloured { edge: edges[0], colour }; count];
    for offset in 0..count {
        let index = (start + offset) % count;
        let next_corner = corners.get(spline + 1) == Some(&index);
        if next_corner {
            spline += 1;
            let last = spline == corners.len() - 1;
            let banned = if last {
                initial
            } else {
                Colour::BLACK
            };
            switch(&mut colour, seed, banned);
        }
        coloured[index] = Coloured { edge: edges[index], colour };
    }
    coloured
}

/// Whether two edges meeting, the first leaving along `before` and the next starting along `after`, both normalized,
/// make a corner: they turn back, or turn by more than [`CORNER`]'s angle.
pub fn is_corner(before: Point, after: Point) -> bool {
    let turns_back = before.dot(after) <= 0.0;
    let turns = before.cross(after).abs() > CORNER;
    turns_back || turns
}

/// Which of three colours edge `index` of `count`, from the corner, takes: the first, middle, or last third, cut
/// symmetrically, as msdfgen's `symmetricalTrichotomy`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a contour's edges are few, and the third is from 0 to 2"
)]
fn trichotomy(index: usize, count: usize) -> usize {
    (2.875 * index as f64 / (count - 1) as f64 + 0.0625).floor().clamp(0.0, 2.0) as usize
}

/// Switches `colour` to another of two channels, unlike `banned` where it can be, as msdfgen's `switchColor`: from
/// white, to a colour `seed` picks; from a colour of two channels, to one sharing one channel with it.
fn switch(colour: &mut Colour, seed: &mut u64, banned: Colour) {
    let shared = colour.and(banned);
    if shared.single() {
        *colour = Colour(shared.0 ^ Colour::WHITE.0);
        return;
    }
    if *colour == Colour::BLACK || *colour == Colour::WHITE {
        let starts = [Colour::CYAN, Colour::MAGENTA, Colour::YELLOW];
        #[allow(clippy::cast_possible_truncation, reason = "a remainder of three fits any width")]
        let pick = (*seed % 3) as usize;
        *colour = starts[pick];
        *seed /= 3;
        return;
    }
    let shifted = u16::from(colour.0) << (1 + (*seed & 1));
    #[allow(clippy::cast_possible_truncation, reason = "masked to three bits")]
    let next = ((shifted | shifted >> 3) & u16::from(Colour::WHITE.0)) as u8;
    *colour = Colour(next);
    *seed >>= 1;
}
