//! Pictures as the library draws them (ADR-0056: Pictures): an image's pixels, decoded once when its bytes arrive and
//! shared from then on, and named by a digest of them, so a screen makes one texture of them whatever widgets show them
//! and compares a picture with the last at the cost of a few numbers.
//!
//! ```text
//! PNG or JPEG bytes ──> Pixels::decode, once ──> Pixels (RGBA, shared) ──> Picture { pixels } ──> the screen's texture
//! ```
//!
//! Decoding is never done per frame: a caller decodes as the bytes arrive, off the view, and hands the view the
//! `Pixels`. Images are untrusted, so a side past [`MOST_SIDE`] or more pixels than [`MOST_PIXELS`] is refused before
//! it is decoded, and nothing here panics on any bytes.

#[cfg(test)]
mod tests;

use aethereal_data::{Bare, Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use core::fmt;
use core::hash::Hasher;
use std::collections::hash_map::DefaultHasher;
use std::io::Cursor;
use std::sync::Arc;
use zune_jpeg::JpegDecoder;
use zune_jpeg::errors::DecodeErrors;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

/// The longest side a picture may have, in pixels: what every GPU the engine draws on holds as a texture's side.
pub const MOST_SIDE: u32 = 8192;

/// The most pixels a picture may have: 64 MiB of RGBA.
pub const MOST_PIXELS: u64 = 16 << 20;

/// An image's pixels: `width` by `height`, red, green, blue, and alpha a byte each, in sRGB, row by row from the top;
/// shared, so a copy costs a count, and named by a digest of them.
#[derive(Clone)]
pub struct Pixels {
    id: PictureId,
    rgba: Arc<[u8]>,
}

/// What names a picture's pixels: its size and a digest of its bytes. Two pictures of one id are drawn from one
/// texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
#[kind(name = "aether.ui.picture_id")]
pub struct PictureId {
    pub width: u32,
    pub height: u32,
    pub digest: u64,
}

/// Why bytes were not made a picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undecoded {
    /// They are neither a PNG nor a JPEG.
    Format,
    /// A side is zero, or longer than [`MOST_SIDE`], or it has more than [`MOST_PIXELS`].
    Size { width: u64, height: u64 },
    /// The pixels are not the bytes the size takes, four for each.
    Bytes { bytes: u64, expected: u64 },
    /// The decoder refused them, as it said.
    Decoder(String),
}

impl Pixels {
    /// The picture of `rgba`, `width` by `height`.
    ///
    /// # Errors
    ///
    /// [`Undecoded::Size`] for a size out of bounds, and [`Undecoded::Bytes`] when `rgba` is not four bytes a pixel.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, Undecoded> {
        checked(u64::from(width), u64::from(height))?;
        let expected = u64::from(width) * u64::from(height) * 4;
        let bytes = rgba.len() as u64;
        if bytes != expected {
            return Err(Undecoded::Bytes { bytes, expected });
        }
        let mut hasher = DefaultHasher::new();
        hasher.write(&rgba);
        let id = PictureId { width, height, digest: hasher.finish() };

        Ok(Self { id, rgba: rgba.into() })
    }

    /// The picture a PNG's or a JPEG's `bytes` hold, told apart by how they start.
    ///
    /// # Errors
    ///
    /// [`Undecoded::Format`] for bytes of neither, [`Undecoded::Size`] for an image out of bounds, read from its header
    /// before its pixels are, and [`Undecoded::Decoder`] for bytes its decoder refuses.
    pub fn decode(bytes: &[u8]) -> Result<Self, Undecoded> {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
        const JPEG: &[u8] = &[0xff, 0xd8, 0xff];

        if bytes.starts_with(PNG) {
            return png(bytes);
        }
        if bytes.starts_with(JPEG) {
            return jpeg(bytes);
        }

        Err(Undecoded::Format)
    }

    /// What names it.
    #[must_use]
    pub const fn id(&self) -> PictureId {
        self.id
    }

    /// Its width and height, in pixels.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.id.width, self.id.height)
    }

    /// Its bytes, four a pixel.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Pictures of one id are the same picture: a digest of a few million bytes differs for any other, and the bytes are
/// compared when two shared copies differ only by where they are kept.
impl PartialEq for Pixels {
    fn eq(&self, other: &Self) -> bool {
        let named = self.id == other.id;
        let shared = Arc::ptr_eq(&self.rgba, &other.rgba);

        named && (shared || self.rgba == other.rgba)
    }
}

impl fmt::Debug for Pixels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Pixels").field("id", &self.id).finish_non_exhaustive()
    }
}

/// A picture is written as its width, its height, and its bytes, and read back by checking them and taking its digest
/// again, so a screen that holds one keeps it across a move onto new code (ADR-0054).
impl Schema for Pixels {
    const SHAPE: Shape = <(u32, u32, Vec<u8>) as Schema>::SHAPE;
}

impl Plain for Pixels where (u32, u32, Vec<u8>): Encode<Bare> + Decode<Bare> {}

impl<B: Boundary> Encode<B> for Pixels
where
    u32: Encode<B>,
    Vec<u8>: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.id.width.encode(out)?;
        self.id.height.encode(out)?;
        self.rgba.to_vec().encode(out)
    }
}

impl<B: Boundary> Decode<B> for Pixels
where
    u32: Decode<B>,
    Vec<u8>: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let width = u32::decode(input)?;
        let height = u32::decode(input)?;
        let rgba = Vec::<u8>::decode(input)?;

        Self::new(width, height, rgba)
            .map_err(|_| DecodeError::Refused { name: "Pixels", rule: "four bytes a pixel, within the most pixels" })
    }
}

/// `Ok` for a picture `width` by `height` within bounds.
fn checked(width: u64, height: u64) -> Result<(), Undecoded> {
    let sides = 1..=u64::from(MOST_SIDE);
    let across = sides.contains(&width);
    let down = sides.contains(&height);
    let few = width.saturating_mul(height) <= MOST_PIXELS;
    let fits = across && down && few;
    if !fits {
        return Err(Undecoded::Size { width, height });
    }

    Ok(())
}

/// The picture a PNG's `bytes` hold, its pixels widened to eight bits a channel and to RGBA, an image without alpha
/// opaque.
fn png(bytes: &[u8]) -> Result<Pixels, Undecoded> {
    let refused = |failed: png::DecodingError| Undecoded::Decoder(failed.to_string());
    let limits = png::Limits { bytes: usize::try_from(MOST_PIXELS * 8).unwrap_or(usize::MAX) };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(refused)?;
    let (width, height) = (reader.info().width, reader.info().height);
    checked(u64::from(width), u64::from(height))?;
    let Some(size) = reader.output_buffer_size() else {
        return Err(Undecoded::Size { width: u64::from(width), height: u64::from(height) });
    };
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(refused)?;
    buffer.truncate(frame.buffer_size());
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb | png::ColorType::Indexed => 3,
        png::ColorType::Rgba => 4,
    };

    Pixels::new(width, height, widened(&buffer, channels))
}

/// The picture a JPEG's `bytes` hold, its pixels as RGBA, opaque.
fn jpeg(bytes: &[u8]) -> Result<Pixels, Undecoded> {
    let refused = |failed: DecodeErrors| Undecoded::Decoder(failed.to_string());
    let most = usize::try_from(MOST_SIDE).unwrap_or(usize::MAX);
    let options =
        DecoderOptions::default().set_max_width(most).set_max_height(most).jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(refused)?;
    let Some(info) = decoder.info() else {
        return Err(Undecoded::Format);
    };
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    checked(u64::from(width), u64::from(height))?;
    let rgba = decoder.decode().map_err(refused)?;

    Pixels::new(width, height, rgba)
}

/// `pixels` of `channels` bytes each, gray, gray and alpha, RGB, or RGBA, as RGBA: gray spread across red, green, and
/// blue, and alpha opaque where there is none.
fn widened(pixels: &[u8], channels: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixels.len() / channels.max(1) * 4);
    for pixel in pixels.chunks_exact(channels.max(1)) {
        let widened = match *pixel {
            [gray] => [gray, gray, gray, u8::MAX],
            [gray, alpha] => [gray, gray, gray, alpha],
            [red, green, blue] => [red, green, blue, u8::MAX],
            [red, green, blue, alpha] => [red, green, blue, alpha],
            _ => [0, 0, 0, 0],
        };
        rgba.extend_from_slice(&widened);
    }

    rgba
}
