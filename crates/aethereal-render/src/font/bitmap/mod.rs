//! Colour glyphs: a font's glyphs as PNG images of one size, its strike, as a colour emoji font holds them (`CBDT`),
//! and each image shrunk to the size the renderer keeps glyphs at. A colour glyph is drawn in its own colours, not a
//! text's, so it is kept as its pixels, not as a distance field: red, green, blue, and alpha, each channel as the PNG
//! encodes it (sRGB) times alpha, so that shrinking and filtering never bleed the colour of a pixel that is not there.

#[cfg(test)]
mod tests;

use super::Drawing;
use core::ops::Range;
use std::io::Cursor;

/// A font's colour glyphs: each glyph's image, by its number, at `pixels_per_em`, and the bytes of every image's PNG,
/// one after another.
#[derive(Debug, Clone)]
pub struct Bitmaps {
    pixels_per_em: u8,
    images: Vec<Image>,
    png: Vec<u8>,
}

/// A glyph's image as a font holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    /// None: the glyph draws nothing, as a space.
    Blank,
    /// A PNG, `bytes` of the font's, set as `metrics` say.
    Png { metrics: BitmapMetrics, bytes: Range<usize> },
}

/// Where a glyph's image is set: its top left, from the glyph's origin, in the strike's pixels, right and up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapMetrics {
    pub left: i8,
    pub top: i8,
}

/// One glyph's image: its PNG, where it is set, and the size of its strike, in pixels to the em.
#[derive(Debug, Clone, Copy)]
pub struct Bitmap<'a> {
    pub png: &'a [u8],
    pub metrics: BitmapMetrics,
    pub pixels_per_em: u8,
}

/// An image shrunk to a size: its pixels, four bytes each, sRGB times alpha, row by row from the top, and where its
/// top left lies from the glyph's origin, in pixels, right and up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shrunk {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub pixels: Vec<u8>,
}

/// Why an image was not shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undecoded {
    /// Its bytes are not a PNG that decodes within [`MOST_DECODED_BYTES`].
    Png,
    /// It decodes to no pixels, or to a form of pixel the decoder was asked not to answer.
    Pixels,
}

/// The most bytes one image may take decoded: 1 MiB, a 512 by 512 image. A colour emoji font's are some 136 by 128.
pub const MOST_DECODED_BYTES: usize = 1024 * 1024;

impl Bitmaps {
    /// The images `images` of a strike of `pixels_per_em`, each glyph's by its number, their PNGs' bytes in `png`.
    pub(super) const fn new(pixels_per_em: u8, images: Vec<Image>, png: Vec<u8>) -> Self {
        Self { pixels_per_em, images, png }
    }

    /// How many glyphs it has an image or a blank for: every glyph of its font.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.images.len()
    }

    /// How the glyph numbered `glyph` is drawn: by its image, or not at all. None for a glyph past the font's.
    #[must_use]
    pub fn drawing(&self, glyph: usize) -> Option<Drawing<'_>> {
        let drawing = match self.images.get(glyph)? {
            Image::Blank => Drawing::Blank,
            Image::Png { metrics, bytes } => {
                let png = self.png.get(bytes.clone())?;
                Drawing::Bitmap(Bitmap { png, metrics: *metrics, pixels_per_em: self.pixels_per_em })
            }
        };

        Some(drawing)
    }
}

impl Bitmap<'_> {
    /// The image shrunk, or grown, to `pixels_per_em`, each pixel the mean of the strike's pixels it covers, weighted
    /// by how much of each it covers, a strike's pixel past the image's edge clear.
    ///
    /// # Errors
    ///
    /// [`Undecoded`] when its PNG does not decode, within [`MOST_DECODED_BYTES`], into pixels.
    pub fn shrunk(&self, pixels_per_em: f32) -> Result<Shrunk, Undecoded> {
        let scale = pixels_per_em / f32::from(self.pixels_per_em);
        let (width, height, pixels) = decoded(self.png)?;
        let columns = spans(width, scale);
        let rows = spans(height, scale);

        let area = scale * scale;
        let mut shrunk = Vec::with_capacity(columns * rows * 4);
        for row in 0..rows {
            for column in 0..columns {
                let mut sum = [0.0f32; 4];
                for (y, down) in span(row, height, scale) {
                    for (x, across) in span(column, width, scale) {
                        let pixel = pixels[(y * width + x) * 4..][..4].iter();
                        for (channel, value) in sum.iter_mut().zip(pixel) {
                            *channel = (f32::from(*value) * down).mul_add(across, *channel);
                        }
                    }
                }
                shrunk.extend(sum.map(|channel| byte(channel * area)));
            }
        }

        let place = |at: i8| whole(f32::from(at) * scale);

        Ok(Shrunk {
            width: u32::try_from(columns).map_err(|_| Undecoded::Pixels)?,
            height: u32::try_from(rows).map_err(|_| Undecoded::Pixels)?,
            left: place(self.metrics.left),
            top: place(self.metrics.top),
            pixels: shrunk,
        })
    }
}

/// The pixels `png` decodes to, four bytes each, sRGB times alpha, beside its width and height.
fn decoded(png: &[u8]) -> Result<(usize, usize, Vec<u8>), Undecoded> {
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(png), png::Limits { bytes: MOST_DECODED_BYTES });
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|_| Undecoded::Png)?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or(Undecoded::Png)?];
    let info = reader.next_frame(&mut buffer).map_err(|_| Undecoded::Png)?;

    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(Undecoded::Pixels),
    };
    let width = usize::try_from(info.width).map_err(|_| Undecoded::Pixels)?;
    let height = usize::try_from(info.height).map_err(|_| Undecoded::Pixels)?;
    let drawn = width > 0 && height > 0;
    if !drawn {
        return Err(Undecoded::Pixels);
    }

    let mut pixels = Vec::with_capacity(width * height * 4);
    for line in buffer.chunks_exact(info.line_size).take(height) {
        let line = line.get(..width * channels).ok_or(Undecoded::Pixels)?;
        for pixel in line.chunks_exact(channels) {
            let [red, green, blue, alpha] = match *pixel {
                [grey] => [grey, grey, grey, u8::MAX],
                [grey, alpha] => [grey, grey, grey, alpha],
                [red, green, blue] => [red, green, blue, u8::MAX],
                [red, green, blue, alpha] => [red, green, blue, alpha],
                _ => return Err(Undecoded::Pixels),
            };
            let times_alpha = |channel: u8| byte(f32::from(channel) * f32::from(alpha) / f32::from(u8::MAX));
            pixels.extend([times_alpha(red), times_alpha(green), times_alpha(blue), alpha]);
        }
    }

    Ok((width, height, pixels))
}

/// How many pixels `length` pixels of the strike become at `scale`: at least one.
// An image is some hundreds of pixels, which an f32 holds exactly, and a scale of a few makes it no larger.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn spans(length: usize, scale: f32) -> usize {
    ((length as f32 * scale).ceil() as usize).max(1)
}

/// Each pixel of `length` pixels of the strike that the pixel `at` of the image at `scale` covers, beside how much of
/// it, from 0 to 1.
// An image is some hundreds of pixels, which an f32 holds exactly, and its pixels' bounds are never negative.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn span(at: usize, length: usize, scale: f32) -> impl Iterator<Item = (usize, f32)> {
    let from = at as f32 / scale;
    let to = ((at + 1) as f32 / scale).min(length as f32);
    let first = from.floor() as usize;
    let past = (to.ceil() as usize).min(length);

    (first..past).map(move |pixel| (pixel, to.min(pixel as f32 + 1.0) - from.max(pixel as f32)))
}

/// `value`, from 0 to 255, rounded to a byte.
// The value is clamped to a byte's range first.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte(value: f32) -> u8 {
    value.round().clamp(0.0, f32::from(u8::MAX)) as u8
}

/// `value` rounded to a whole number of pixels.
// A strike's metrics are a byte, which a scale of a few keeps far inside an i32.
#[allow(clippy::cast_possible_truncation)]
fn whole(value: f32) -> i32 {
    value.round() as i32
}
