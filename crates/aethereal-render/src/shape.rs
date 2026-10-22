//! The shape of what a pass draws into: its colour's format, its samples, and its depth. A program is made against a
//! shape, and fits every pass of that shape in any frame, so a frame replaced by one whose passes keep their shapes
//! keeps every program (ADR-0025).

use aethereal_data::Plain;

/// What a pass draws into, as a program is made against it: the format of its one colour target, how many samples
/// each pixel holds, and the depth it holds beside its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.shape")]
pub struct Shape {
    pub colour: ColourFormat,
    pub samples: Samples,
    pub depth: DepthFormat,
}

impl Shape {
    /// The shape of a pass drawing into `colour` of one sample, with the depth every program tests against.
    #[must_use]
    pub const fn with_depth(colour: ColourFormat) -> Self {
        Self { colour, samples: Samples::One, depth: DepthFormat::Depth32Float }
    }

    /// The shape of a pass drawing into `colour` of one sample, holding no depth: an effect's, or a draw's that uses
    /// none.
    #[must_use]
    pub const fn flat(colour: ColourFormat) -> Self {
        Self { colour, samples: Samples::One, depth: DepthFormat::None }
    }
}

/// The format of a pass's colour target: a window's, or a texture's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.colour_format")]
pub enum ColourFormat {
    /// Red, green, blue, and alpha, a byte each, read as fractions.
    Rgba8,
    /// Red, green, blue, and alpha, a byte each, the colours encoded as sRGB.
    Rgba8Srgb,
    /// Blue, green, red, and alpha, a byte each, as a browser's canvas often prefers.
    Bgra8,
    /// Blue, green, red, and alpha, a byte each, the colours encoded as sRGB.
    Bgra8Srgb,
    /// Red, green, blue, and alpha, a 16-bit float each: a frame's target of floats.
    Rgba16Float,
    /// One 32-bit whole number: a frame's target of whole numbers, which no program blends into.
    R32Uint,
    /// One byte, read as a fraction: a frame's end in [`TextureFormat::R8`](crate::texture::TextureFormat::R8).
    R8,
}

impl ColourFormat {
    /// The format, as wgpu names it.
    #[must_use]
    pub const fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            Self::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::Bgra8 => wgpu::TextureFormat::Bgra8Unorm,
            Self::Bgra8Srgb => wgpu::TextureFormat::Bgra8UnormSrgb,
            Self::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
            Self::R32Uint => wgpu::TextureFormat::R32Uint,
            Self::R8 => wgpu::TextureFormat::R8Unorm,
        }
    }

    /// Whether it holds whole numbers, which a fragment stage writes as `u32` and nothing blends.
    #[must_use]
    pub const fn is_whole(self) -> bool {
        matches!(self, Self::R32Uint)
    }

    /// The format wgpu names `format`: none for one no pass draws into.
    #[must_use]
    pub const fn of(format: wgpu::TextureFormat) -> Option<Self> {
        match format {
            wgpu::TextureFormat::Rgba8Unorm => Some(Self::Rgba8),
            wgpu::TextureFormat::Rgba8UnormSrgb => Some(Self::Rgba8Srgb),
            wgpu::TextureFormat::Bgra8Unorm => Some(Self::Bgra8),
            wgpu::TextureFormat::Bgra8UnormSrgb => Some(Self::Bgra8Srgb),
            wgpu::TextureFormat::Rgba16Float => Some(Self::Rgba16Float),
            wgpu::TextureFormat::R32Uint => Some(Self::R32Uint),
            wgpu::TextureFormat::R8Unorm => Some(Self::R8),
            _ => None,
        }
    }
}

/// How many samples a target holds of each pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.samples")]
pub enum Samples {
    /// One: nothing is multisampled.
    One,
    /// Four, as WebGPU multisamples: resolved to one as each pass that draws into a target of them ends (ADR-0025).
    Four,
}

impl Samples {
    /// How many samples, as wgpu counts them.
    #[must_use]
    pub const fn count(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Four => 4,
        }
    }
}

/// The depth a pass holds beside its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.depth_format")]
pub enum DepthFormat {
    /// None: a program drawing there neither tests nor writes depth.
    None,
    /// A 32-bit float for each pixel.
    Depth32Float,
}
