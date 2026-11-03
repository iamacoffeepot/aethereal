//! What `capture_frame` does to a frame's pixels: cut to its crop, shrunk in linear light, and written as a PNG
//! (ADR-0043).

use aethereal_data::Plain;
use core::array;

/// A part of a frame, in its pixels, from its top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.region")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    /// The whole of a frame `width` by `height`.
    pub const fn whole(width: u32, height: u32) -> Self {
        Self { x: 0, y: 0, width, height }
    }

    /// Whether this is a part of a frame `width` by `height`: not empty, and not past its edge.
    pub fn within(self, width: u32, height: u32) -> bool {
        let is_empty = self.width == 0 || self.height == 0;
        let fits_across = self.x.checked_add(self.width).is_some_and(|right| right <= width);
        let fits_down = self.y.checked_add(self.height).is_some_and(|bottom| bottom <= height);

        !is_empty && fits_across && fits_down
    }
}

/// The size an image `width` by `height` is sent at: as it is when its longer side is at most `longest`, and never
/// enlarged; else shrunk to keep its shape, its longer side `longest` and its shorter at least one pixel.
pub fn sent_size(width: u32, height: u32, longest: u32) -> (u32, u32) {
    let longer = width.max(height);
    if longer <= longest {
        return (width, height);
    }
    let scaled = |side: u32| {
        let rounded = (u64::from(side) * u64::from(longest) + u64::from(longer) / 2) / u64::from(longer);
        u32::try_from(rounded).unwrap_or(longest).max(1)
    };

    (scaled(width), scaled(height))
}

/// The RGB bytes of `region` of `pixels`, a frame's RGBA rows `frame_width` wide, row by row from its top: the alpha
/// dropped, as a frame's alpha is how its pass left it, not what its window shows.
pub fn cut(pixels: &[u8], frame_width: u32, region: Region) -> Vec<u8> {
    let (row, left, across) = (frame_width as usize * 4, region.x as usize * 4, region.width as usize * 4);
    let mut rgb = Vec::with_capacity(region.width as usize * region.height as usize * 3);
    for line in pixels.chunks_exact(row).skip(region.y as usize).take(region.height as usize) {
        let cut = line.get(left..left + across).unwrap_or_default();
        rgb.extend(cut.chunks_exact(4).flat_map(|pixel| pixel.iter().take(3).copied()));
    }

    rgb
}

/// `rgb`, `width` by `height`, shrunk to `to_width` by `to_height`, each no larger: each pixel the mean, in linear
/// light, of the pixels it covers, each weighted by how much of it is covered, so a thin bright line on a dark ground
/// keeps its brightness rather than darkening as an average of encoded values would. An image of its own size is
/// answered as it is.
pub fn shrunk(rgb: &[u8], width: u32, height: u32, to_width: u32, to_height: u32) -> Vec<u8> {
    let is_same = width == to_width && height == to_height;
    if is_same {
        return rgb.to_vec();
    }
    let linear: [f64; 256] = array::from_fn(|byte| decoded(u8::try_from(byte).unwrap_or(u8::MAX)));
    let (across, down) = (spans(width, to_width), spans(height, to_height));
    let (width, to_width) = (width as usize, to_width as usize);

    let mut rows = vec![0.0; height as usize * to_width * 3];
    for (y, row) in rgb.chunks_exact(width * 3).enumerate() {
        for (x, span) in across.iter().enumerate() {
            for channel in 0..3 {
                let mean = span.iter().map(|&(at, weight)| linear[usize::from(row[at * 3 + channel])] * weight).sum();
                rows[(y * to_width + x) * 3 + channel] = mean;
            }
        }
    }

    let mut out = Vec::with_capacity(to_width * to_height as usize * 3);
    for span in &down {
        for x in 0..to_width {
            for channel in 0..3 {
                let mean: f64 = span.iter().map(|&(at, weight)| rows[(at * to_width + x) * 3 + channel] * weight).sum();
                out.push(encoded(mean));
            }
        }
    }

    out
}

/// For each of `to` pixels across a line of `from`, the pixels of the line it covers and the share of it each covers:
/// pixel `at` of the line spans `[at * to, (at + 1) * to)` and pixel `out` of the result `[out * from, (out + 1) *
/// from)`, in units of `1 / to` of a pixel of the line, so every overlap is a whole number of them.
fn spans(from: u32, to: u32) -> Vec<Vec<(usize, f64)>> {
    let (from, to) = (u64::from(from), u64::from(to));
    (0..to)
        .map(|out| {
            let (start, end) = (out * from, (out + 1) * from);
            let (first, last) = (start / to, end.div_ceil(to));
            (first..last)
                .map(|at| {
                    let overlap = end.min((at + 1) * to) - start.max(at * to);
                    let share =
                        u32::try_from(overlap).map_or(0.0, f64::from) / u32::try_from(from).map_or(1.0, f64::from);
                    (usize::try_from(at).unwrap_or(usize::MAX), share)
                })
                .collect()
        })
        .collect()
}

/// The light a byte of sRGB stands for, from zero to one.
fn decoded(byte: u8) -> f64 {
    let encoded = f64::from(byte) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The byte of sRGB that stands for `light`, from zero to one.
fn encoded(light: f64) -> u8 {
    let light = light.clamp(0.0, 1.0);
    let encoded = if light <= 0.003_130_8 {
        light * 12.92
    } else {
        1.055f64.mul_add(light.powf(1.0 / 2.4), -0.055)
    };
    // Clamped to between zero and one, so the rounded value is a byte.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (encoded * 255.0).round() as u8;

    byte
}

/// `rgb`, `width` by `height`, written as a PNG into `buffer`, emptied first: eight bits a channel, marked as sRGB,
/// as a frame's bytes are as its window shows them, at the encoder's fast compression.
pub fn png(rgb: &[u8], width: u32, height: u32, mut buffer: Vec<u8>) -> Result<Vec<u8>, png::EncodingError> {
    buffer.clear();
    let mut encoder = png::Encoder::new(&mut buffer, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;

    Ok(buffer)
}
