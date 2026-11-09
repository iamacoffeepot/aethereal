//! `loca` and `glyf`: each glyph's outline. A simple glyph is contours of points on and off its quadratic curves; a
//! composite glyph is other glyphs placed, each transformed and moved. A composite is resolved here into contours of
//! its own, by a stack rather than recursion, nested no deeper than [`MOST_NESTED`].

use super::head::LocaOffsets;
use super::{Data, FontRefused};
use crate::font::outline::{Outline, Point};
use aethereal_math::Vec2;
use std::iter;

/// How deep composite glyphs may nest: a glyph of glyphs of glyphs, and so on. Fonts nest two or three deep; a glyph
/// nested deeper, or one that holds itself, refuses its font.
pub const MOST_NESTED: usize = 8;

/// How many points one glyph may have, its components' counted: TrueType numbers a glyph's points in 16 bits.
const MOST_POINTS: usize = 65_535;

/// A simple glyph's flag: the point is on the curve, not a control point.
const ON_CURVE: u8 = 0x01;
/// A simple glyph's flag: the point's `x` is one byte, unsigned.
const X_SHORT: u8 = 0x02;
/// A simple glyph's flag: the point's `y` is one byte, unsigned.
const Y_SHORT: u8 = 0x04;
/// A simple glyph's flag: the flag holds for as many more points as the next byte says.
const REPEAT: u8 = 0x08;
/// A simple glyph's flag: with [`X_SHORT`], the byte is positive; without it, `x` is the point before's.
const X_SAME_OR_POSITIVE: u8 = 0x10;
/// A simple glyph's flag: with [`Y_SHORT`], the byte is positive; without it, `y` is the point before's.
const Y_SAME_OR_POSITIVE: u8 = 0x20;

/// A component's flag: its two arguments are 16-bit, not 8-bit.
const ARGUMENT_WORDS: u16 = 0x0001;
/// A component's flag: its arguments are an offset, not two points to join.
const ARGUMENTS_OFFSET: u16 = 0x0002;
/// A component's flag: one scale for both axes follows.
const SCALE: u16 = 0x0008;
/// A component's flag: another component follows this one.
const MORE_COMPONENTS: u16 = 0x0020;
/// A component's flag: a scale for each axis follows.
const XY_SCALE: u16 = 0x0040;
/// A component's flag: a 2×2 matrix follows.
const TWO_BY_TWO: u16 = 0x0080;

/// Reads the outline of each of `glyph_count` glyphs, composites resolved.
pub fn read(
    loca: Data<'_>,
    glyf: Data<'_>,
    offsets: LocaOffsets,
    glyph_count: u16,
) -> Result<Vec<Outline>, FontRefused> {
    let glyphs = Glyphs::read(loca, glyf, offsets, glyph_count)?;
    (0..glyph_count).map(|glyph| glyphs.resolve(glyph).map(|contours| Outline::from_points(&contours))).collect()
}

/// Where each glyph is in `glyf`.
struct Glyphs<'a> {
    glyf: Data<'a>,
    /// Each glyph's offset into `glyf`, and one past the last glyph: a glyph runs to the next glyph's offset.
    offsets: Vec<usize>,
}

/// A glyph as `glyf` holds it.
enum Glyph {
    /// A glyph that draws nothing, as a space.
    Empty,
    /// Contours of points.
    Simple(Vec<Vec<Point>>),
    /// Other glyphs, placed.
    Composite(Vec<Component>),
}

/// A glyph placed in a composite.
#[derive(Debug, Clone, Copy)]
struct Component {
    glyph: u16,
    transform: Transform,
    anchor: Anchor,
}

/// A linear map of a component's points: `x` along `x_axis` and `y` along `y_axis`.
#[derive(Debug, Clone, Copy)]
struct Transform {
    x_axis: Vec2,
    y_axis: Vec2,
}

/// Where a component goes once transformed.
#[derive(Debug, Clone, Copy)]
enum Anchor {
    /// Moved by an offset, in font units.
    Offset(Vec2),
    /// Moved so its point `child` lands on point `parent` of the contours placed before it.
    Points { parent: usize, child: usize },
}

/// A composite being resolved: its components, how many are placed, and the contours they made.
struct Frame {
    components: Vec<Component>,
    placed: usize,
    contours: Vec<Vec<Point>>,
    points: usize,
}

impl<'a> Glyphs<'a> {
    /// Reads `loca`'s offsets for `glyph_count` glyphs.
    fn read(loca: Data<'_>, glyf: Data<'a>, offsets: LocaOffsets, glyph_count: u16) -> Result<Self, FontRefused> {
        let offsets = (0..=usize::from(glyph_count))
            .map(|index| match offsets {
                LocaOffsets::Short => loca.u16(index * 2).map(|half| usize::from(half) * 2),
                LocaOffsets::Long => loca.offset32(index * 4),
            })
            .collect::<Result<Vec<_>, FontRefused>>()?;
        let ordered = offsets.windows(2).all(|pair| pair[0] <= pair[1]);
        if !ordered {
            return Err(loca.malformed());
        }

        Ok(Self { glyf, offsets })
    }

    /// Reads `glyph` as `glyf` holds it.
    fn glyph(&self, glyph: u16) -> Result<Glyph, FontRefused> {
        let refused = self.glyf.malformed();
        let index = usize::from(glyph);
        let start = *self.offsets.get(index).ok_or(refused)?;
        let end = *self.offsets.get(index + 1).ok_or(refused)?;
        let data = self.glyf.slice(start, end - start)?;
        if data.is_empty() {
            return Ok(Glyph::Empty);
        }

        let contours = data.i16(0)?;
        if contours < 0 {
            return composite(data).map(Glyph::Composite);
        }
        simple(data, usize::from(contours.unsigned_abs())).map(Glyph::Simple)
    }

    /// Resolves `glyph` into contours of its own: a composite's components each read, transformed, and moved, in
    /// order, those that are composites themselves resolved first.
    fn resolve(&self, glyph: u16) -> Result<Vec<Vec<Point>>, FontRefused> {
        let refused = self.glyf.malformed();
        let components = match self.glyph(glyph)? {
            Glyph::Empty => return Ok(Vec::new()),
            Glyph::Simple(contours) => return Ok(contours),
            Glyph::Composite(components) => components,
        };

        let mut stack = vec![Frame::new(components)];
        while let Some(top) = stack.last_mut() {
            let next = top.components.get(top.placed).map(|component| component.glyph);
            if let Some(next) = next {
                match self.glyph(next)? {
                    Glyph::Empty => top.place(Vec::new(), refused)?,
                    Glyph::Simple(contours) => top.place(contours, refused)?,
                    Glyph::Composite(components) => {
                        let deep = stack.len() >= MOST_NESTED;
                        if deep {
                            return Err(refused);
                        }
                        stack.push(Frame::new(components));
                    }
                }
                continue;
            }

            let Some(resolved) = stack.pop() else {
                break;
            };
            match stack.last_mut() {
                Some(parent) => parent.place(resolved.contours, refused)?,
                None => return Ok(resolved.contours),
            }
        }

        Err(refused)
    }
}

impl Frame {
    /// A composite of `components`, none placed.
    const fn new(components: Vec<Component>) -> Self {
        Self { components, placed: 0, contours: Vec::new(), points: 0 }
    }

    /// Places the next component, whose glyph resolved into `contours`.
    fn place(&mut self, contours: Vec<Vec<Point>>, refused: FontRefused) -> Result<(), FontRefused> {
        let component = *self.components.get(self.placed).ok_or(refused)?;
        let transformed = contours
            .into_iter()
            .map(|contour| contour.into_iter().map(|point| component.transform.apply(point)).collect::<Vec<_>>())
            .collect::<Vec<_>>();

        let offset = match component.anchor {
            Anchor::Offset(offset) => offset,
            Anchor::Points { parent, child } => {
                let parent = self.contours.iter().flatten().nth(parent).ok_or(refused)?;
                let child = transformed.iter().flatten().nth(child).ok_or(refused)?;
                parent.at - child.at
            }
        };

        self.points += transformed.iter().map(Vec::len).sum::<usize>();
        if self.points > MOST_POINTS {
            return Err(refused);
        }
        let moved = transformed
            .into_iter()
            .map(|contour| contour.into_iter().map(|point| Point { at: point.at + offset, ..point }).collect());
        self.contours.extend(moved);
        self.placed += 1;

        Ok(())
    }
}

impl Transform {
    /// The transform that leaves points where they are.
    const IDENTITY: Self = Self { x_axis: Vec2::X, y_axis: Vec2::Y };

    /// `point`, transformed.
    fn apply(self, point: Point) -> Point {
        Point { at: self.x_axis * point.at.x + self.y_axis * point.at.y, ..point }
    }
}

/// Reads a simple glyph of `count` contours.
fn simple(data: Data<'_>, count: usize) -> Result<Vec<Vec<Point>>, FontRefused> {
    let ends = (0..count).map(|contour| data.offset16(10 + contour * 2)).collect::<Result<Vec<_>, FontRefused>>()?;
    let increasing = ends.windows(2).all(|pair| pair[0] < pair[1]);
    if !increasing {
        return Err(data.malformed());
    }
    let point_count = ends.last().map_or(0, |last| last + 1);

    let instructions = data.offset16(10 + count * 2)?;
    let mut at = 12 + count * 2 + instructions;
    let mut flags = Vec::new();
    while flags.len() < point_count {
        let flag = data.u8(at)?;
        at += 1;
        let repeated = flag & REPEAT != 0;
        let repeats = if repeated {
            usize::from(data.u8(at)?)
        } else {
            0
        };
        at += usize::from(repeated);
        flags.extend(iter::repeat_n(flag, repeats + 1));
    }
    if flags.len() > point_count {
        return Err(data.malformed());
    }

    let xs = coordinates(data, &mut at, &flags, X_SHORT, X_SAME_OR_POSITIVE)?;
    let ys = coordinates(data, &mut at, &flags, Y_SHORT, Y_SAME_OR_POSITIVE)?;
    let points = flags
        .iter()
        .zip(xs.into_iter().zip(ys))
        .map(|(flag, (x, y))| Point { at: Vec2::new(f32::from(x), f32::from(y)), on_curve: flag & ON_CURVE != 0 })
        .collect::<Vec<_>>();

    let refused = data.malformed();
    let mut start = 0;
    ends.iter()
        .map(|&end| {
            let contour = points.get(start..=end).ok_or(refused)?.to_vec();
            start = end + 1;
            Ok(contour)
        })
        .collect()
}

/// Reads one axis of a simple glyph's points, from `at`, each a change from the point before.
fn coordinates(data: Data<'_>, at: &mut usize, flags: &[u8], short: u8, same: u8) -> Result<Vec<i16>, FontRefused> {
    let refused = data.malformed();
    let mut value: i16 = 0;
    flags
        .iter()
        .map(|flag| {
            let change = match (flag & short != 0, flag & same != 0) {
                (true, positive) => {
                    let byte = i16::from(data.u8(*at)?);
                    *at += 1;
                    if positive {
                        byte
                    } else {
                        -byte
                    }
                }
                (false, true) => 0,
                (false, false) => {
                    let word = data.i16(*at)?;
                    *at += 2;
                    word
                }
            };
            value = value.checked_add(change).ok_or(refused)?;
            Ok(value)
        })
        .collect()
}

/// Reads a composite glyph's components.
fn composite(data: Data<'_>) -> Result<Vec<Component>, FontRefused> {
    let mut at = 10;
    let mut components = Vec::new();
    loop {
        let flags = data.u16(at)?;
        let glyph = data.u16(at + 2)?;
        at += 4;

        let words = flags & ARGUMENT_WORDS != 0;
        let offset = flags & ARGUMENTS_OFFSET != 0;
        let anchor = match (words, offset) {
            (true, true) => Anchor::Offset(Vec2::new(f32::from(data.i16(at)?), f32::from(data.i16(at + 2)?))),
            (true, false) => Anchor::Points { parent: data.offset16(at)?, child: data.offset16(at + 2)? },
            (false, true) => Anchor::Offset(Vec2::new(f32::from(data.i8(at)?), f32::from(data.i8(at + 1)?))),
            (false, false) => {
                Anchor::Points { parent: usize::from(data.u8(at)?), child: usize::from(data.u8(at + 1)?) }
            }
        };
        at += if words {
            4
        } else {
            2
        };

        let transform = if flags & SCALE != 0 {
            let scale = fraction(data, at)?;
            at += 2;
            Transform { x_axis: Vec2::X * scale, y_axis: Vec2::Y * scale }
        } else if flags & XY_SCALE != 0 {
            let transform =
                Transform { x_axis: Vec2::X * fraction(data, at)?, y_axis: Vec2::Y * fraction(data, at + 2)? };
            at += 4;
            transform
        } else if flags & TWO_BY_TWO != 0 {
            let x_axis = Vec2::new(fraction(data, at)?, fraction(data, at + 2)?);
            let y_axis = Vec2::new(fraction(data, at + 4)?, fraction(data, at + 6)?);
            at += 8;
            Transform { x_axis, y_axis }
        } else {
            Transform::IDENTITY
        };

        components.push(Component { glyph, transform, anchor });
        if flags & MORE_COMPONENTS == 0 {
            return Ok(components);
        }
    }
}

/// Reads the 2.14 fixed-point number at `at`, as a component's transform holds.
fn fraction(data: Data<'_>, at: usize) -> Result<f32, FontRefused> {
    data.i16(at).map(|value| f32::from(value) / 16384.0)
}
