//! Reading back: the frame just drawn, a texture, a region of a frame's target, or storage, copied off the GPU and
//! answered as a blob, once the host has mapped the copy (ADR-0019, ADR-0023, ADR-0026, ADR-0048).

#[cfg(test)]
mod tests;

use crate::frame::{Frame, Scale, Size, TargetKind};
use crate::storage::StorageId;
use crate::texture::{TextureFormat, TextureId};
use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};
use aethereal_window::WindowId;

/// Asks the renderer for what `of` holds, as bytes. Answered with [`Pixels`] once the GPU has copied it and the host
/// has mapped the copy, a frame or more later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_back")]
pub struct ReadBack {
    pub of: Readable,
}

/// What is read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.readable")]
pub enum Readable {
    /// The next frame drawn onto the window, as it is when its pass ends.
    Frame(WindowId),
    /// A texture the renderer keeps, as it is when the ask is handled.
    Texture(TextureId),
    /// The last frame drawn onto the window, as it was when its pass ended, read when the ask is handled: what a
    /// held clock's window shows, as no frame is drawn until it is advanced (ADR-0045).
    Last(WindowId),
}

/// What the renderer answers [`ReadBack`] with.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.pixels")]
pub enum Pixels {
    /// The pixels, `width` by `height` of `format`, row by row from the top, each row packed tight: a pixel of
    /// `Rgba16Float` is its four 16-bit floats, little-endian, as the GPU holds them.
    Read {
        width: u32,
        height: u32,
        format: TextureFormat,
        pixels: Bytes,
    },
    Refused(ReadRefused),
}

/// Asks the renderer for the bytes `storage` holds, as it is when the ask is handled. Any actor handed it may read it.
/// Answered with [`StorageContents`] once the GPU has copied it and the host has mapped the copy, a frame or more later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_storage")]
pub struct ReadStorage {
    pub storage: StorageId,
}

/// What the renderer answers [`ReadStorage`] with.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.storage_contents")]
pub enum StorageContents {
    /// The storage's bytes, all of them, as the GPU holds them.
    Read {
        bytes: Bytes,
    },
    Refused(ReadRefused),
}

/// Asks the renderer for what the target at `target`, its place in the frame's `targets`, holds within `region`, in the
/// pixels of the frame's end, as the last `Render` left it: answered with [`TargetPixels`] once the GPU has copied it
/// and the host has mapped the copy (ADR-0048).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_frame_target")]
pub struct ReadFrameTarget {
    pub frame: Frame,
    pub target: u32,
    pub region: Region,
}

/// A rectangle of pixels: its top left, `x` and `y` from the top left, and its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.region")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    /// The region of a target of `scale` that covers this one of an end of `end`, cut to the end: each side divided
    /// as the scale divides the end's, the top left rounded down and the far edge up. None when none of it is within
    /// the end.
    #[must_use]
    pub const fn scaled(self, end: Size, scale: Scale) -> Option<Self> {
        let right = min(self.x.saturating_add(self.width), end.width);
        let bottom = min(self.y.saturating_add(self.height), end.height);
        let is_empty = right <= self.x || bottom <= self.y;
        if is_empty {
            return None;
        }
        let by = scale.divisor();
        let (x, y) = (self.x / by, self.y / by);

        Some(Self { x, y, width: right.div_ceil(by) - x, height: bottom.div_ceil(by) - y })
    }
}

/// The lesser of `a` and `b`, in a `const fn`, where `Ord::min` is not.
const fn min(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
}

/// What the renderer answers [`ReadFrameTarget`] with.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.target_pixels")]
pub enum TargetPixels {
    /// The pixels of `region`, in the target's pixels, row by row from the top, each row packed tight, laid out as
    /// `kind` holds them: a float target's pixel its four 16-bit floats, a whole number's its `u32`, and depth's its
    /// `f32`, from 0 at the near plane to 1 at the far, each little-endian, as the GPU holds them.
    Read {
        region: Region,
        kind: TargetKind,
        pixels: Bytes,
    },
    Refused(ReadRefused),
}

/// How many bytes a pixel of a target of `kind` takes, as it is copied.
pub(crate) const fn target_bytes(kind: TargetKind) -> u32 {
    match kind {
        TargetKind::Float => 8,
        TargetKind::Uint | TargetKind::Depth => 4,
    }
}

/// Why what was asked for was not read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_refused")]
pub enum ReadRefused {
    /// No texture is kept under the id: never made, or released.
    Texture,
    /// No storage is kept under the id: never made, or released.
    Storage,
    /// No window is drawn to under the id: never opened, or closed, before its frame was drawn.
    Window,
    /// The canvas is of a format no answer is laid out in: not four channels of a byte each.
    Format,
    /// The canvas was not opened to be copied from, as the chassis opens it when the GPU lets it.
    Uncopied,
    /// The GPU did not map the copy back.
    Unmapped,
    /// The store refused the bytes, as they are more than this worker may put.
    Unstored,
    /// No frame of the window is kept: none has been drawn onto it, or its surface cannot be copied from.
    Undrawn,
    /// No frame is the window's main frame, or kept under the id: never made, released, or the window closed.
    Frame,
    /// The frame has no target at the place asked.
    Target,
    /// The target holds four samples of depth, which the GPU copies no texture of.
    Multisampled,
    /// None of the region is within the frame's end.
    Outside,
}

/// The order of the channels of a pixel the GPU copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// As the format the answer names lays them out.
    Kept,
    /// Blue, green, red, alpha: the red and blue are swapped into the answer's red, green, blue, alpha.
    Bgra,
}

/// The format a frame drawn to the canvas's `format` is answered in, and the order its channels are copied in: none
/// for a canvas of a format no answer is laid out in.
pub(crate) const fn frame_format(format: wgpu::TextureFormat) -> Option<(TextureFormat, Order)> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => Some((TextureFormat::Rgba8, Order::Kept)),
        wgpu::TextureFormat::Rgba8UnormSrgb => Some((TextureFormat::Rgba8Srgb, Order::Kept)),
        wgpu::TextureFormat::Bgra8Unorm => Some((TextureFormat::Rgba8, Order::Bgra)),
        wgpu::TextureFormat::Bgra8UnormSrgb => Some((TextureFormat::Rgba8Srgb, Order::Bgra)),
        _ => None,
    }
}

/// How a texture is laid out as the GPU copies it into a buffer: its size, the bytes of a pixel, and the bytes of a
/// row in the buffer, padded to the 256 WebGPU copies rows on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Copied {
    pub width: u32,
    pub height: u32,
    pub pixel: u32,
    pub row: u32,
}

impl Copied {
    /// The layout of a texture `width` by `height`, of pixels of `pixel` bytes.
    pub(crate) const fn new(width: u32, height: u32, pixel: u32) -> Self {
        let tight = width * pixel;
        let row = tight.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

        Self { width, height, pixel, row }
    }

    /// The bytes of the buffer the texture is copied into.
    pub(crate) fn bytes(self) -> u64 {
        u64::from(self.row) * u64::from(self.height)
    }

    /// The pixels of `within`, a region of what was copied so into `mapped`, row by row, each row packed tight.
    pub(crate) fn cut(self, mapped: &[u8], within: Region) -> Vec<u8> {
        let pixel = self.pixel as usize;
        let (left, width) = (within.x as usize * pixel, within.width as usize * pixel);

        mapped
            .chunks(self.row as usize)
            .skip(within.y as usize)
            .take(within.height as usize)
            .flat_map(|row| row.iter().skip(left).take(width).copied())
            .collect()
    }

    /// The pixels of `mapped`, the buffer a texture was copied into so, each row cut to its pixels and the channels of
    /// each put in the answer's order.
    pub(crate) fn unpad(self, mapped: &[u8], order: Order) -> Vec<u8> {
        let tight = (self.width * self.pixel) as usize;
        let mut pixels: Vec<u8> =
            mapped.chunks(self.row as usize).flat_map(|row| row.iter().take(tight).copied()).collect();
        if order == Order::Bgra {
            for pixel in pixels.chunks_exact_mut(self.pixel as usize) {
                pixel.swap(0, 2);
            }
        }

        pixels
    }
}
