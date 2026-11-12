//! How a screen's draws reach the renderer (ADR-0056: Draws are kept by widget key): each widget's draws in layers,
//! a run of quads sampling one texture drawn by one placement of the library's program, and each text a text of the
//! renderer's own (ADR-0052). A layer is kept by its widget's key and its place among the widget's layers, so a layout
//! that changes it sends only what changed: a write of its records, a write or a move of its text.
//!
//! The program draws a unit square for each record: a box in the screen's pixels, turned into clip space by the size
//! of the frame it draws in, filled with its colour or a texture it tints, rounded at its corners, with a border inside
//! its edge, and cut to a clip of its own. The clip is compared with the position the program placed, in the screen's
//! own pixels, never with the fragment's position in the frame: a call that places the screen in its caller's
//! coordinates moves what the program placed after it is placed (ADR-0055), and the clip moves with it.

#[cfg(test)]
mod tests;

use crate::widget::{Piece, Quad, Sample};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::mesh::{Attribute, VertexFormat, VertexLayout};
use aethereal_render::text::{Align, Font, Outline, Run, Wrap};
use aethereal_render::texture::TextureId;

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

/// The program's vertex stage's entry point.
pub const PLACE: &str = "place";

/// The program's fragment stage's entry point.
pub const SHADE: &str = "shade";

/// How many bytes one record takes: eight vectors of four floats.
pub const RECORD_BYTES: u32 = 128;

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

/// The program's uniform for a frame `width` by `height` pixels.
#[must_use]
pub fn frame_bytes(width: u32, height: u32) -> Vec<u8> {
    // A frame's sides are far fewer pixels than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let size = [width as f32, height as f32, 0.0, 0.0];

    size.iter().flat_map(|value| value.to_le_bytes()).collect()
}

/// The record of `quad`, as the program reads it.
#[must_use]
pub fn record(quad: &Quad) -> Vec<u8> {
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

    vectors.iter().flatten().flat_map(|value| value.to_le_bytes()).collect()
}

/// What a quad layer's placement binds as its texture: the library's white pixel, which a solid quad samples, or a
/// texture an image names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Binding {
    White,
    Texture(TextureId),
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

/// A widget's draws, `pieces`, in its layers: each run of quads that sample one texture, a solid quad sampling the
/// white pixel, in one layer, and each text in a layer of its own, in the order they were drawn.
pub fn layered(pieces: Vec<Piece>) -> Vec<Content> {
    let mut layers: Vec<Content> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Quad(quad) => {
                let binding = match quad.sample {
                    Sample::Solid => Binding::White,
                    Sample::Texture { texture, .. } => Binding::Texture(texture),
                };
                let record = record(&quad);
                match layers.last_mut() {
                    Some(Content::Quads { binding: last, records }) if *last == binding => records.extend(record),
                    Some(Content::Quads { .. } | Content::Text { .. }) | None => {
                        layers.push(Content::Quads { binding, records: record });
                    }
                }
            }
            Piece::Text(text) => {
                let shape = Shape {
                    font: text.font,
                    size: text.size,
                    wrap: text.wrap,
                    align: text.align,
                    outline: text.outline,
                };
                let placing = Placing { at: text.at, clip: text.clip };
                layers.push(Content::Text { shape, runs: text.runs, placing });
            }
        }
    }

    layers
}
