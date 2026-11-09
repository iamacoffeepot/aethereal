//! The job that generates glyphs' distance fields at the base size, off the render root's turn, as `capture_frame`
//! encodes its image (ADR-0043, ADR-0052): run by the render root alone, with the outlines of the glyphs a text needs
//! that the atlas has not, and answered with each field, which the render root writes into the atlas.

use crate::font::msdf::{self, BASE_PIXELS_PER_EM, DEFAULT_PADDING};
use crate::font::outline::{Contour, Outline, Segment};
use aethereal_actor::{Bytes, Fault, Job, Runner, Running};
use aethereal_data::{Bound, Plain};
use aethereal_math::Vec2;

/// Generates the field of each of `glyphs`, in order, at [`BASE_PIXELS_PER_EM`] with [`DEFAULT_PADDING`].
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.generate_glyphs")]
pub struct GenerateGlyphs {
    pub glyphs: Vec<GlyphOutline>,
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

/// What the job answers with: each glyph's field, in the order asked, and their texels, three bytes each, one field's
/// after another's.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.glyphs_generated")]
pub struct GlyphsGenerated {
    pub fields: Vec<GeneratedField>,
    pub pixels: Bytes,
}

/// A field generated: its texels, and where its top left lies from the glyph's origin, right and up, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.generated_field")]
pub struct GeneratedField {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
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
        let (fields, pixels) = generate(&self.glyphs);

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

/// The field of each of `glyphs`, in order, at the base size, and their texels, one field's after another's.
#[must_use]
pub fn generate(glyphs: &[GlyphOutline]) -> (Vec<GeneratedField>, Vec<u8>) {
    let mut fields = Vec::with_capacity(glyphs.len());
    let mut pixels = Vec::new();
    for glyph in glyphs {
        let field = msdf::generate(&glyph.outline(), glyph.pixels_per_unit, DEFAULT_PADDING);
        fields.push(GeneratedField { width: field.width, height: field.height, left: field.left, top: field.top });
        pixels.extend(field.pixels);
    }

    (fields, pixels)
}
