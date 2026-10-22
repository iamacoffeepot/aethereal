//! Reading back: the frame just drawn, a texture, or storage, copied off the GPU and answered as a blob, once the host
//! has mapped the copy (ADR-0019, ADR-0023, ADR-0026).

#[cfg(test)]
mod tests;

use crate::storage::StorageId;
use crate::texture::{TextureFormat, TextureId};
use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Plain};

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
    /// The next frame drawn onto the canvas, as it is when its pass ends.
    Frame,
    /// A texture the renderer keeps, as it is when the ask is handled.
    Texture(TextureId),
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
        pixels: BlobRef,
    },
    Refused(ReadRefused),
}

/// Asks the renderer for the bytes `storage` holds, as it is when the ask is handled. Any actor handed it may read it.
/// Answered with [`Bytes`] once the GPU has copied it and the host has mapped the copy, a frame or more later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_storage")]
pub struct ReadStorage {
    pub storage: StorageId,
}

/// What the renderer answers [`ReadStorage`] with.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.bytes")]
pub enum Bytes {
    /// The storage's bytes, all of them, as the GPU holds them.
    Read {
        bytes: BlobRef,
    },
    Refused(ReadRefused),
}

/// Why what was asked for was not read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_refused")]
pub enum ReadRefused {
    /// No texture is kept under the id: never made, or released.
    Texture,
    /// No storage is kept under the id: never made, or released.
    Storage,
    /// The canvas is of a format no answer is laid out in: not four channels of a byte each.
    Format,
    /// The canvas was not opened to be copied from, as the chassis opens it when the GPU lets it.
    Uncopied,
    /// The GPU did not map the copy back.
    Unmapped,
    /// The store refused the bytes, as they are more than this worker may put.
    Unstored,
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
