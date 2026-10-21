//! Textures: images a program samples, asked for by any actor and checked as they are made (ADR-0021).

#[cfg(test)]
mod tests;

use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Kind, KindName, Plain};

/// Asks the renderer for a texture `width` by `height` of `format`, from `pixels`, row by row from the top. Answered
/// with [`Textured`].
#[derive(Bound)]
pub struct CreateTexture {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub pixels: BlobRef,
}

impl Kind for CreateTexture {
    const NAME: KindName = KindName::new("aether.render.create_texture");
}

/// The format of a texture's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum TextureFormat {
    /// Red, green, blue, and alpha, a byte each, read as they are.
    Rgba8,
    /// Red, green, blue, and alpha, a byte each, the colours encoded as sRGB and read back to linear.
    Rgba8Srgb,
    /// One byte, read as a fraction from zero to one.
    R8,
}

impl Kind for TextureFormat {
    const NAME: KindName = KindName::new("aether.render.texture_format");
}

impl TextureFormat {
    /// How many bytes a pixel of this format takes.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Rgba8Srgb => 4,
            Self::R8 => 1,
        }
    }

    /// The format, as wgpu names it.
    #[must_use]
    pub const fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            Self::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::R8 => wgpu::TextureFormat::R8Unorm,
        }
    }
}

/// A texture the renderer made, which any actor handed it may draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct TextureId {
    pub(crate) id: u64,
}

impl Kind for TextureId {
    const NAME: KindName = KindName::new("aether.render.texture_id");
}

/// What the renderer answers [`CreateTexture`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Textured {
    Made(TextureId),
    Refused(TextureRefused),
}

impl Kind for Textured {
    const NAME: KindName = KindName::new("aether.render.textured");
}

/// Why a texture was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum TextureRefused {
    /// A side is zero, or longer than the GPU's longest.
    Size { width: u32, height: u32, most: u32 },
    /// The pixels are not the bytes the size and format take.
    Pixels { bytes: u64, expected: u64 },
}

impl Kind for TextureRefused {
    const NAME: KindName = KindName::new("aether.render.texture_refused");
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
