//! Textures: images a program samples, asked for by any actor and checked as they are made (ADR-0021).

#[cfg(test)]
mod tests;

use crate::shape::ColourFormat;
use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Plain};

/// Asks the renderer for a texture `width` by `height` of `format`, from `pixels`, row by row from the top. Answered
/// with [`Textured`].
#[derive(Bound)]
#[kind(name = "aether.render.create_texture")]
pub struct CreateTexture {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub pixels: BlobRef,
}

/// The format of a texture's pixels. Every one is read by a program as filtered floats, `texture_2d<f32>`, and every
/// one is drawn into by a frame that ends on a texture of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.texture_format")]
pub enum TextureFormat {
    /// Red, green, blue, and alpha, a byte each, read as they are.
    Rgba8,
    /// Red, green, blue, and alpha, a byte each, the colours encoded as sRGB and read back to linear.
    Rgba8Srgb,
    /// One byte, read as a fraction from zero to one.
    R8,
    /// Red, green, blue, and alpha, a 16-bit float each, little-endian: values past one are kept, as a glow kept
    /// between frames needs.
    Rgba16Float,
}

impl TextureFormat {
    /// How many bytes a pixel of this format takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Rgba8Srgb => 4,
            Self::R8 => 1,
            Self::Rgba16Float => 8,
        }
    }

    /// The format, as wgpu names it.
    #[must_use]
    pub const fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            Self::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::R8 => wgpu::TextureFormat::R8Unorm,
            Self::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        }
    }

    /// The colour format a pass draws a texture of this format in, as a frame that ends on one does.
    #[must_use]
    pub const fn colour(self) -> ColourFormat {
        match self {
            Self::Rgba8 => ColourFormat::Rgba8,
            Self::Rgba8Srgb => ColourFormat::Rgba8Srgb,
            Self::R8 => ColourFormat::R8,
            Self::Rgba16Float => ColourFormat::Rgba16Float,
        }
    }
}

/// A texture the renderer made, which any actor handed it may draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.texture_id")]
pub struct TextureId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateTexture`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.textured")]
pub enum Textured {
    Made(TextureId),
    Refused(TextureRefused),
}

/// Why a texture was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.texture_refused")]
pub enum TextureRefused {
    /// A side is zero, or longer than the GPU's longest.
    Size { width: u32, height: u32, most: u32 },
    /// The pixels are not the bytes the size and format take.
    Pixels { bytes: u64, expected: u64 },
}

/// Checks a texture before it is made: each side is at least one pixel and at most `most`, the GPU's longest, and
/// `pixels` are the bytes its size and format take.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(width: u32, height: u32, format: TextureFormat, pixels: &[u8], most: u32) -> Result<(), TextureRefused> {
    let fits = (1..=most).contains(&width) && (1..=most).contains(&height);
    if !fits {
        return Err(TextureRefused::Size { width, height, most });
    }
    let expected = u64::from(width) * u64::from(height) * u64::from(format.bytes());
    let bytes = pixels.len() as u64;
    if bytes != expected {
        return Err(TextureRefused::Pixels { bytes, expected });
    }

    Ok(())
}
