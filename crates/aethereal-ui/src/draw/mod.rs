//! How a screen's draws reach the renderer (ADR-0056: Draws are kept by widget key): each widget's draws in layers,
//! a run of quads sampling one texture drawn by one placement of the library's program, and each text a text of the
//! renderer's own (ADR-0052). A layer is kept by its widget's key and its place among the widget's layers, so a layout
//! that changes it sends only what changed: a write of its records, a write or a move of its text.
//!
//! A run of solid quads that are all square and borderless, as a chart's columns and a panel's fill are, is drawn by
//! the library's second program, [`FILL_PROGRAM`], from records a quarter the size: each its box, already cut to its
//! clip, and its colour. A widget that redraws hundreds of quads each frame then writes 32 bytes for each, not 128.
//!
//! The program draws a unit square for each record: a box in the screen's pixels, turned into clip space by the size
//! of the frame it draws in, filled with its colour or a texture it tints, rounded at its corners, with a border inside
//! its edge, and cut to a clip of its own. The clip is compared with the position the program placed, in the screen's
//! own pixels, never with the fragment's position in the frame: a call that places the screen in its caller's
//! coordinates moves what the program placed after it is placed (ADR-0055), and the clip moves with it.

#[cfg(test)]
mod tests;

use crate::widget::{Piece, Quad, Sample, TextDraw};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::mesh::{Attribute, VertexFormat, VertexLayout};
use aethereal_render::text::{Align, Font, Outline, Run, Wrap};
use aethereal_render::texture::TextureId;
use core::ops::Range;

/// The program that draws every quad of a screen: the unit square's corner placed in its record's box and turned into
/// clip space by the frame's size in the uniform; then the texture sampled at the record's region, its edges kept by
/// the record's slices, tinted by its colour, under its border, rounded at its corners, and cut to its clip.
pub const PROGRAM: &str = "
@group(0) @binding(0) var<uniform> frame: vec4<f32>;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var sampling: sampler;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) colour: vec4<f32>,
    @location(4) @interpolate(flat) border_colour: vec4<f32>,
    @location(5) @interpolate(flat) shape: vec4<f32>,
    @location(6) @interpolate(flat) uv: vec4<f32>,
    @location(7) @interpolate(flat) inset: vec4<f32>,
    @location(8) @interpolate(flat) slice_uv: vec4<f32>,
}

@vertex
fn place(
    @location(0) corner: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) clip: vec4<f32>,
    @location(3) colour: vec4<f32>,
    @location(4) border_colour: vec4<f32>,
    @location(5) shape: vec4<f32>,
    @location(6) uv: vec4<f32>,
    @location(7) inset: vec4<f32>,
    @location(8) slice_uv: vec4<f32>,
) -> Between {
    let pixel = mix(rect.xy, rect.zw, corner);
    let placed = vec2(pixel.x / frame.x * 2.0 - 1.0, 1.0 - pixel.y / frame.y * 2.0);
    return Between(vec4(placed, 0.0, 1.0), pixel, rect, clip, colour, border_colour, shape, uv, inset, slice_uv);
}

// Where a point `along` pixels into a quad `size` pixels long samples, from `low` to `high` of the texture: within
// `near` pixels of its start, the band of `near_uv` there, unstretched; within `far` of its end, the band of `far_uv`
// there; and between them, what lies between the bands, stretched.
fn sliced(along: f32, size: f32, near: f32, far: f32, low: f32, high: f32, near_uv: f32, far_uv: f32) -> f32 {
    if along < near {
        return low + along / near * near_uv;
    }
    if along > size - far {
        return high - (size - along) / far * far_uv;
    }
    let middle = max(size - near - far, 0.000001);
    return low + near_uv + (along - near) / middle * (high - low - near_uv - far_uv);
}

// How far `point` lies outside a box `half` from its middle each way, its corners rounded by `radius`: negative inside.
fn outside(point: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let rounded = min(radius, min(half.x, half.y));
    let corner = abs(point) - half + vec2(rounded);
    return length(max(corner, vec2(0.0))) + min(max(corner.x, corner.y), 0.0) - rounded;
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let along = between.local - between.rect.xy;
    let size = between.rect.zw - between.rect.xy;
    let u = sliced(along.x, size.x, between.inset.x, between.inset.z, between.uv.x, between.uv.z, between.slice_uv.x, between.slice_uv.z);
    let v = sliced(along.y, size.y, between.inset.y, between.inset.w, between.uv.y, between.uv.w, between.slice_uv.y, between.slice_uv.w);
    let fill = textureSample(picture, sampling, vec2(u, v)) * between.colour;

    let half = size * 0.5;
    let distance = outside(along - half, half, between.shape.y);
    let covered = clamp(0.5 - distance, 0.0, 1.0);
    let inner = clamp(0.5 - (distance + between.shape.x), 0.0, 1.0);
    var shaded = fill;
    if between.shape.x > 0.0 {
        shaded = mix(between.border_colour, fill, inner);
    }

    let clip = between.clip;
    let cut = between.local.x < clip.x || between.local.y < clip.y || between.local.x >= clip.z || between.local.y >= clip.w;
    if cut {
        discard;
    }
    return vec4(shaded.rgb, shaded.a * covered);
}
";

/// The program that draws a run of plain fills: the unit square's corner placed in its record's box, already cut to
/// its clip, and turned into clip space by the frame's size in the uniform; then the record's colour, its edges eased
/// over a pixel as the library's program eases a square quad's.
pub const FILL_PROGRAM: &str = "
@group(0) @binding(0) var<uniform> frame: vec4<f32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @location(1) rect: vec4<f32>, @location(2) colour: vec4<f32>) -> Between {
    let pixel = mix(rect.xy, rect.zw, corner);
    let placed = vec2(pixel.x / frame.x * 2.0 - 1.0, 1.0 - pixel.y / frame.y * 2.0);
    return Between(vec4(placed, 0.0, 1.0), pixel, rect, colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let half = (between.rect.zw - between.rect.xy) * 0.5;
    let corner = abs(between.local - between.rect.xy - half) - half;
    let distance = length(max(corner, vec2(0.0))) + min(max(corner.x, corner.y), 0.0);
    let covered = clamp(0.5 - distance, 0.0, 1.0);
    return vec4(between.colour.rgb, between.colour.a * covered);
}
";

/// The programs' vertex stage's entry point.
pub const PLACE: &str = "place";

/// The programs' fragment stage's entry point.
pub const SHADE: &str = "shade";

/// How many bytes one record takes: eight vectors of four floats.
pub const RECORD_BYTES: u32 = 128;

/// How many bytes one fill's record takes: its box and its colour, four floats each.
pub const FILL_BYTES: u32 = 32;

/// How many bytes the program's uniform takes: the frame's width and height in pixels, and two floats it does not
/// read.
pub const FRAME_BYTES: u32 = 16;

/// The unit square's layout: one corner of two floats.
#[must_use]
pub fn corner_layout() -> VertexLayout {
    VertexLayout { stride: 8, attributes: vec![Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 }] }
}

/// The unit square's corners, from its top left, clockwise on screen.
#[must_use]
pub fn corners() -> Vec<u8> {
    [0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0].iter().flat_map(|corner| corner.to_le_bytes()).collect()
}

/// The square's two triangles, indexing its corners.
#[must_use]
pub fn triangles() -> Vec<u8> {
    [0_u32, 1, 2, 0, 2, 3].iter().flat_map(|index| index.to_le_bytes()).collect()
}

/// How a record is laid out: its box, its clip, its colour, its border's colour, its border's width and its corners'
/// radius, the region of the texture it samples, the bands its slices keep in pixels, and in the texture's share.
#[must_use]
pub fn record_layout() -> VertexLayout {
    let attributes = (1..=8_u32)
        .zip((0..).step_by(16))
        .map(|(location, offset)| Attribute { location, format: VertexFormat::Float32x4, offset })
        .collect();

    VertexLayout { stride: RECORD_BYTES, attributes }
}

/// How a fill's record is laid out: its box, cut to its clip, then its colour.
#[must_use]
pub fn fill_layout() -> VertexLayout {
    let rect = Attribute { location: 1, format: VertexFormat::Float32x4, offset: 0 };
    let colour = Attribute { location: 2, format: VertexFormat::Float32x4, offset: 16 };

    VertexLayout { stride: FILL_BYTES, attributes: vec![rect, colour] }
}

/// The program's uniform for a frame `width` by `height` pixels.
#[must_use]
pub fn frame_bytes(width: u32, height: u32) -> Vec<u8> {
    // A frame's sides are far fewer pixels than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let size = [width as f32, height as f32, 0.0, 0.0];

    size.iter().flat_map(|value| value.to_le_bytes()).collect()
}

/// Writes the record of `quad`, as the program reads it, after what `records` holds.
pub fn record(quad: &Quad, records: &mut Vec<u8>) {
    let Quad { rect, clip, colour, border_width, border_colour, radius, sample } = *quad;
    let (uv, inset, slice_uv) = match sample {
        Sample::Solid => (Rect2 { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0) }, [0.0; 4], [0.0; 4]),
        Sample::Texture { uv, slices, .. } => {
            let inset = [slices.inset.left, slices.inset.top, slices.inset.right, slices.inset.bottom];
            let slice_uv = [slices.uv.left, slices.uv.top, slices.uv.right, slices.uv.bottom];
            (uv, inset, slice_uv)
        }
    };
    let corners = |rect: Rect2| [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
    let vectors = [
        corners(rect),
        corners(clip),
        colour.channels(),
        border_colour.channels(),
        [border_width, radius, 0.0, 0.0],
        corners(uv),
        inset,
        slice_uv,
    ];

    write::<{ RECORD_BYTES as usize }>(records, vectors.as_flattened());
}

/// Writes `floats` after what `records` holds, each in its four little-endian bytes, gathered first so they are
/// written as one run.
fn write<const BYTES: usize>(records: &mut Vec<u8>, floats: &[f32]) {
    let mut bytes = [0_u8; BYTES];
    for (place, value) in bytes.chunks_exact_mut(4).zip(floats) {
        place.copy_from_slice(&value.to_le_bytes());
    }

    records.extend_from_slice(&bytes);
}

/// Writes the record of `quad`, a plain fill, as the fill program reads it, after what `records` holds: its box cut to
/// its clip, and its colour; nothing when the clip leaves nothing of it.
pub fn fill(quad: &Quad, records: &mut Vec<u8>) {
    let cut = quad.rect.intersection(quad.clip);
    if cut.is_empty() {
        return;
    }
    let vectors = [[cut.min.x, cut.min.y, cut.max.x, cut.max.y], quad.colour.channels()];

    write::<{ FILL_BYTES as usize }>(records, vectors.as_flattened());
}

/// Whether `quad` is a plain fill: solid, square, and borderless, so its box, its clip, and its colour say all of it.
fn is_plain(quad: &Quad) -> bool {
    let solid = matches!(quad.sample, Sample::Solid);
    let square = quad.radius <= 0.0;
    let borderless = quad.border_width <= 0.0;

    solid && square && borderless
}

/// What a quad layer's placement draws with: the fill program, binding the frame's size alone; or the library's
/// program, binding as its texture the library's white pixel, which a solid quad samples, or a texture an image names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Binding {
    Fill,
    White,
    Texture(TextureId),
}

impl Binding {
    /// How many bytes one of its records takes.
    #[must_use]
    pub const fn record_bytes(self) -> u32 {
        match self {
            Self::Fill => FILL_BYTES,
            Self::White | Self::Texture(_) => RECORD_BYTES,
        }
    }

    /// How its records are laid out.
    #[must_use]
    pub fn layout(self) -> VertexLayout {
        match self {
            Self::Fill => fill_layout(),
            Self::White | Self::Texture(_) => record_layout(),
        }
    }
}

/// What one layer of a widget's draws is: quads drawn by one placement, their records as the program reads them; or a
/// text, as it is made, what it shows, and where it is placed.
#[derive(Debug, Clone, PartialEq, Plain)]
pub enum Content {
    Quads { binding: Binding, records: Vec<u8> },
    Text { shape: Shape, runs: Vec<Run>, placing: Placing },
}

/// What a text is made with that a write does not change: a change to any of it makes the text anew.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Shape {
    pub font: Font,
    pub size: f32,
    pub wrap: Wrap,
    pub align: Align,
    pub outline: Outline,
}

/// Where a text is placed: its first line's top left, and what it is cut to, in its screen's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Placing {
    pub at: Vec2,
    pub clip: Rect2,
}

/// A widget's draws, `pieces`, in its layers, taken out of `pieces`: each run of quads that sample one texture, a solid
/// quad sampling the white pixel, in one layer, and each text in a layer of its own, in the order they were drawn. A
/// run of solid quads that are all plain fills is drawn by the fill program. Each layer's records are written into one
/// buffer of the size they take, so a widget that draws hundreds of quads makes no buffer for each.
pub fn layered(pieces: &mut Vec<Piece>) -> Vec<Content> {
    let mut quads: Vec<Quad> = Vec::with_capacity(pieces.len());
    let mut runs: Vec<Gathered> = Vec::new();
    for piece in pieces.drain(..) {
        match piece {
            Piece::Quad(quad) => {
                let binding = match quad.sample {
                    Sample::Solid => Binding::White,
                    Sample::Texture { texture, .. } => Binding::Texture(texture),
                };
                let at = quads.len();
                quads.push(quad);
                match runs.last_mut() {
                    Some(Gathered::Quads { binding: last, span }) if *last == binding => span.end = at + 1,
                    Some(Gathered::Quads { .. } | Gathered::Text(_)) | None => {
                        runs.push(Gathered::Quads { binding, span: at..at + 1 });
                    }
                }
            }
            Piece::Text(text) => runs.push(Gathered::Text(text)),
        }
    }

    runs.into_iter().map(|run| run.content(&quads)).collect()
}

/// A run of a widget's draws that one layer draws: quads of one binding, the span of them among the widget's quads, or
/// a text.
enum Gathered {
    Quads { binding: Binding, span: Range<usize> },
    Text(TextDraw),
}

impl Gathered {
    /// The layer that draws it, its quads among `quads`: their records, as fills when they are all plain fills; or its
    /// text.
    fn content(self, quads: &[Quad]) -> Content {
        match self {
            Self::Quads { binding, span } => {
                let quads = &quads[span];
                let plain = quads.iter().all(is_plain);
                let binding = if plain {
                    Binding::Fill
                } else {
                    binding
                };
                let mut records = Vec::with_capacity(quads.len() * binding.record_bytes() as usize);
                for quad in quads {
                    if plain {
                        fill(quad, &mut records);
                    } else {
                        record(quad, &mut records);
                    }
                }

                Content::Quads { binding, records }
            }
            Self::Text(text) => {
                let shape = Shape {
                    font: text.font,
                    size: text.size,
                    wrap: text.wrap,
                    align: text.align,
                    outline: text.outline,
                };
                let placing = Placing { at: text.at, clip: text.clip };

                Content::Text { shape, runs: text.runs, placing }
            }
        }
    }
}
