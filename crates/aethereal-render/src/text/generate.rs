//! The job that generates glyphs at the base size, off the render root's turn, as `capture_frame` encodes its image
//! (ADR-0043, ADR-0052): run by the render root alone, with the outline or the image of each glyph a text needs that
//! the atlas has not, and answered with each glyph's texels, which the render root writes into the atlas: an outline's
//! distance field, or an image shrunk, in its own colours.

use crate::font::bitmap::{Bitmap, BitmapMetrics};
use crate::font::msdf::{self, BASE_PIXELS_PER_EM, DEFAULT_PADDING};
use crate::font::outline::{Contour, Outline, Segment};
use aethereal_actor::{Bytes, Fault, Job, Runner, Running};
use aethereal_data::{Bound, Plain};
use aethereal_math::Vec2;

/// Generates each of `glyphs`, in order, at [`BASE_PIXELS_PER_EM`]: an outline's field with [`DEFAULT_PADDING`], or
/// an image shrunk, its PNG in `images`, the PNGs of the batch's images one after another.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.generate_glyphs")]
pub struct GenerateGlyphs {
    pub glyphs: Vec<GlyphSource>,
    pub images: Bytes,
}

/// What a glyph is generated from.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.glyph_source")]
pub enum GlyphSource {
    /// Its outline: a distance field, drawn in a text's colour.
    Outline(GlyphOutline),
    /// Its image: drawn in its own colours.
    Bitmap(GlyphBitmap),
    /// Nothing: a glyph of a colour font with no image, as a space, which has no texels.
    Blank,
}

/// A glyph's outline, in font units, and how many pixels a font unit is at the base size, as its font says.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.glyph_outline")]
pub struct GlyphOutline {
    pub pixels_per_unit: f32,
    pub contours: Vec<OutlineContour>,
}

/// One closed contour of a glyph's outline.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.outline_contour")]
pub struct OutlineContour {
    pub segments: Vec<OutlineSegment>,
}

/// A piece of a contour: a line, or a quadratic curve.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.outline_segment")]
pub enum OutlineSegment {
    Line { from: Vec2, to: Vec2 },
    Quadratic { from: Vec2, control: Vec2, to: Vec2 },
}

/// A glyph's image: where its PNG is in its batch's images, `length` bytes from `start`, the size of its font's strike
/// in pixels to the em, and where its top left is set from the glyph's origin, in the strike's pixels, right and up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.glyph_bitmap")]
pub struct GlyphBitmap {
    pub start: u32,
    pub length: u32,
    pub pixels_per_em: u8,
    pub left: i8,
    pub top: i8,
}

/// What the job answers with: each glyph generated, in the order asked, and their texels, four bytes each, one
/// glyph's after another's.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.glyphs_generated")]
pub struct GlyphsGenerated {
    pub fields: Vec<GeneratedField>,
    pub pixels: Bytes,
}

/// A glyph generated: its texels, where its top left lies from the glyph's origin, right and up, in pixels, and how
/// they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.generated_field")]
pub struct GeneratedField {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub ink: Ink,
}

/// How a glyph's texels in the atlas are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.ink")]
pub enum Ink {
    /// As a distance field in red, green, and blue, its alpha opaque: the glyph is drawn in its text's colour.
    Field,
    /// As the glyph's own colours, sRGB times alpha: drawn as they are, only its text's alpha and opacity applied.
    Colour,
}

/// What the render root keeps for a job it ran: which of its batches of glyphs it generates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.generating")]
pub struct Generating {
    pub batch: u64,
}

/// It computes, on threads for computing, so no `Render` waits on a glyph.
impl Job for GenerateGlyphs {
    type Done = GlyphsGenerated;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<GlyphsGenerated, Fault> {
        let (fields, pixels) = generate(&self.glyphs, job.read(&self.images));

        Ok(GlyphsGenerated { fields, pixels: job.put(pixels) })
    }
}

impl GlyphOutline {
    /// The outline `outline`, of a font of `units_per_em`, as the job carries it.
    #[must_use]
    pub fn of(outline: &Outline, units_per_em: u16) -> Self {
        let contours = outline
            .contours
            .iter()
            .map(|contour| OutlineContour {
                segments: contour.segments.iter().map(|segment| carried(*segment)).collect(),
            })
            .collect();

        Self { pixels_per_unit: BASE_PIXELS_PER_EM / f32::from(units_per_em), contours }
    }

    /// The outline it carries.
    fn outline(&self) -> Outline {
        let contours = self
            .contours
            .iter()
            .map(|contour| Contour { segments: contour.segments.iter().map(|segment| drawn(*segment)).collect() })
            .collect();

        Outline { contours }
    }
}

impl GlyphBitmap {
    /// The image `bitmap`, as the job carries it, its PNG appended to `images`, its batch's: none when the PNG would
    /// end past the 4 GiB a batch's images may hold.
    #[must_use]
    pub fn of(bitmap: Bitmap<'_>, images: &mut Vec<u8>) -> Option<Self> {
        let Bitmap { png, metrics, pixels_per_em } = bitmap;
        let start = u32::try_from(images.len()).ok()?;
        let length = u32::try_from(png.len()).ok()?;
        start.checked_add(length)?;
        images.extend_from_slice(png);

        Some(Self { start, length, pixels_per_em, left: metrics.left, top: metrics.top })
    }

    /// The image it carries, its PNG read from `images`, its batch's: none when they do not hold it.
    fn bitmap(self, images: &[u8]) -> Option<Bitmap<'_>> {
        let start = usize::try_from(self.start).ok()?;
        let png = images.get(start..start.checked_add(usize::try_from(self.length).ok()?)?)?;
        let metrics = BitmapMetrics { left: self.left, top: self.top };

        Some(Bitmap { png, metrics, pixels_per_em: self.pixels_per_em })
    }
}

/// `segment`, as the job carries it.
const fn carried(segment: Segment) -> OutlineSegment {
    match segment {
        Segment::Line { from, to } => OutlineSegment::Line { from, to },
        Segment::Quadratic { from, control, to } => OutlineSegment::Quadratic { from, control, to },
    }
}

/// The segment the job carries as `segment`.
const fn drawn(segment: OutlineSegment) -> Segment {
    match segment {
        OutlineSegment::Line { from, to } => Segment::Line { from, to },
        OutlineSegment::Quadratic { from, control, to } => Segment::Quadratic { from, control, to },
    }
}

/// Each of `glyphs` generated, in order, at the base size, the PNGs of their images in `images`, and their texels, four
/// bytes each, one glyph's after another's. An image `images` does not hold, or that does not decode, has no texels,
/// and draws nothing.
#[must_use]
pub fn generate(glyphs: &[GlyphSource], images: &[u8]) -> (Vec<GeneratedField>, Vec<u8>) {
    let mut fields = Vec::with_capacity(glyphs.len());
    let mut pixels = Vec::new();
    for glyph in glyphs {
        let blank = GeneratedField { width: 0, height: 0, left: 0, top: 0, ink: Ink::Colour };
        let field = match glyph {
            GlyphSource::Outline(outline) => {
                let field = msdf::generate(&outline.outline(), outline.pixels_per_unit, DEFAULT_PADDING);
                pixels.extend(field.pixels.chunks_exact(3).flat_map(|texel| [texel[0], texel[1], texel[2], u8::MAX]));
                GeneratedField {
                    width: field.width,
                    height: field.height,
                    left: field.left,
                    top: field.top,
                    ink: Ink::Field,
                }
            }
            GlyphSource::Bitmap(bitmap) => match bitmap.bitmap(images).map(|held| held.shrunk(BASE_PIXELS_PER_EM)) {
                Some(Ok(shrunk)) => {
                    pixels.extend(shrunk.pixels);
                    GeneratedField {
                        width: shrunk.width,
                        height: shrunk.height,
                        left: shrunk.left,
                        top: shrunk.top,
                        ink: Ink::Colour,
                    }
                }
                Some(Err(_)) | None => blank,
            },
            GlyphSource::Blank => blank,
        };
        fields.push(field);
    }

    (fields, pixels)
}
