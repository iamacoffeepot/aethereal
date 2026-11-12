//! `Image`: a texture, or a region of one, drawn over its box, tinted by its style's colour, its edges kept unstretched
//! by a nine-slice (ADR-0056: Stock widgets). Its content is the region's size in logical pixels, a texel to each.

use crate::fonts::Measuring;
use crate::style::{Edges, Style};
use crate::widget::{Arrange, Draws, Laid, Offer, Quad, Sample, Slices, Stateless, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::texture::TextureId;

/// A texture the renderer made, with its size in texels, which the region it is drawn from is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    pub id: TextureId,
    pub width: u32,
    pub height: u32,
}

/// A region of a texture, in its texels from its top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The region `region` of `texture`, its bands `slices` texels in from each edge kept unstretched as it is drawn larger
/// or smaller: its corners unscaled, its edges stretched along their length alone, and its middle stretched to fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Image {
    pub texture: Texture,
    pub region: Region,
    pub slices: Edges<u32>,
}

impl Image {
    /// The whole of `texture`, stretched.
    #[must_use]
    pub const fn whole(texture: Texture) -> Self {
        Self {
            texture,
            region: Region { x: 0, y: 0, width: texture.width, height: texture.height },
            slices: Edges::all(0),
        }
    }
}

/// Texels as floats: a texture's sides are far fewer texels than an f32 counts exactly.
#[allow(clippy::cast_precision_loss)]
const fn texels(count: u32) -> f32 {
    count as f32
}

/// Bands `near` and `far` pixels deep at each end of a side `length` pixels long, shrunk together to fit it when they
/// would overlap.
fn fitted(near: f32, far: f32, length: f32) -> (f32, f32) {
    let both = near + far;
    let overlap = both > length && both > 0.0;
    if !overlap {
        return (near, far);
    }
    let share = length.max(0.0) / both;

    (near * share, far * share)
}

impl Widget for Image {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, _: &Style, measuring: &mut Measuring<'_>, _: Offer) -> Vec2 {
        Vec2::new(texels(self.region.width), texels(self.region.height)) * measuring.scale()
    }

    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let across = texels(self.texture.width.max(1));
        let down = texels(self.texture.height.max(1));
        let Region { x, y, width, height } = self.region;
        let uv = Rect2 {
            min: Vec2::new(texels(x) / across, texels(y) / down),
            max: Vec2::new(texels(x + width) / across, texels(y + height) / down),
        };
        let box_ = laid.content;
        let (left, right) =
            fitted(texels(self.slices.left) * laid.scale, texels(self.slices.right) * laid.scale, box_.width());
        let (top, bottom) =
            fitted(texels(self.slices.top) * laid.scale, texels(self.slices.bottom) * laid.scale, box_.height());
        let slices = Slices {
            inset: Edges { left, right, top, bottom },
            uv: Edges {
                left: texels(self.slices.left) / across,
                right: texels(self.slices.right) / across,
                top: texels(self.slices.top) / down,
                bottom: texels(self.slices.bottom) / down,
            },
        };
        draws.quad(Quad {
            rect: box_,
            clip: laid.clip,
            colour: style.colour,
            border_width: 0.0,
            border_colour: style.colour,
            radius: style.radius * laid.scale,
            sample: Sample::Texture { texture: self.texture.id, uv, slices },
        });
    }
}
