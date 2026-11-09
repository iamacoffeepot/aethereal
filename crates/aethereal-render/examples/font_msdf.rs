//! Writes PNGs for eyes to judge the font module by (ADR-0052): a few glyphs' multi-channel distance fields at the base
//! size, and two lines of text drawn from those fields at 13 and 48 pixels to the em, each cut at 0.5 and then
//! smoothed over a pixel as the glyph program will. Prints how long reading the font and generating a field take.
//!
//! `cargo run --release -p aethereal-render --features tools --example font_msdf -- <directory>`

#![allow(clippy::print_stdout)] // A measure prints its timings.
#![allow(clippy::disallowed_methods)] // A native measure owns the real clock (ADR-0001).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)] // A picture's pixels.

use aethereal_render::font::msdf::{BASE_PIXELS_PER_EM, DEFAULT_PADDING, Msdf};
use aethereal_render::font::{Font, GlyphId};
use std::collections::HashMap;
use std::error::Error;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs, io};

const NOTO: &[u8] = include_bytes!("../fixtures/fonts/NotoSans-Regular.ttf");

/// The glyphs whose fields are drawn as they are.
const GLYPHS: &str = "gW@\u{e9}AO&";

/// The text drawn from the fields.
const TEXT: &str = "Sphinx of black quartz, judge my vow.\nAV To Wa \u{e9}t\u{e9} @home g 0123456789";

/// How many times each timing is taken.
const ROUNDS: u32 = 50;

/// How large each field's texels are drawn in the picture of the fields, and the small text magnified.
const MAGNIFIED: usize = 4;

fn main() -> Result<(), Box<dyn Error>> {
    let directory = env::args().nth(1).map_or_else(|| PathBuf::from("font-msdf"), PathBuf::from);
    fs::create_dir_all(&directory)?;
    let font = Font::read(NOTO).map_err(|refused| format!("the font was refused: {refused:?}"))?;

    time(&font);
    fields(&font, &directory.join("glyphs.png"))?;

    let fields = TEXT
        .chars()
        .filter_map(|character| font.glyph(character))
        .chain([GlyphId::MISSING])
        .filter_map(|glyph| Some((glyph, font.msdf(glyph, BASE_PIXELS_PER_EM, DEFAULT_PADDING)?)))
        .collect::<HashMap<_, _>>();
    for size in [13.0, 48.0] {
        let (width, height, grey) = text(&font, &fields, size);
        let name = format!("text_{size}px.png");
        png(&directory.join(name), width, height, png::ColorType::Grayscale, &grey)?;
        let (magnified, grey) = magnify(width, &grey, 1, MAGNIFIED);
        let name = format!("text_{size}px_x{MAGNIFIED}.png");
        png(&directory.join(name), magnified, height * MAGNIFIED, png::ColorType::Grayscale, &grey)?;
    }

    Ok(())
}

/// Prints how long reading the font takes, and generating the field of a few glyphs at the base size.
fn time(font: &Font) {
    let started = Instant::now();
    for _ in 0..ROUNDS {
        black_box(Font::read(black_box(NOTO)).ok());
    }
    let read = started.elapsed() / ROUNDS;
    println!("read {} glyphs in {:.2} ms", font.glyph_count(), read.as_secs_f64() * 1000.0);

    for character in ['g', 'W', '@'] {
        let Some(glyph) = font.glyph(character) else {
            continue;
        };
        let started = Instant::now();
        for _ in 0..ROUNDS {
            black_box(font.msdf(black_box(glyph), BASE_PIXELS_PER_EM, DEFAULT_PADDING));
        }
        let generated = started.elapsed() / ROUNDS;
        let field =
            font.msdf(glyph, BASE_PIXELS_PER_EM, DEFAULT_PADDING).map_or((0, 0), |msdf| (msdf.width, msdf.height));
        println!(
            "generated {character:?} ({}x{} texels) in {:.3} ms",
            field.0,
            field.1,
            generated.as_secs_f64() * 1000.0
        );
    }
}

/// Writes the fields of [`GLYPHS`] side by side, magnified, on grey.
fn fields(font: &Font, path: &Path) -> Result<(), Box<dyn Error>> {
    let fields = GLYPHS
        .chars()
        .filter_map(|character| font.msdf(font.glyph(character)?, BASE_PIXELS_PER_EM, DEFAULT_PADDING))
        .collect::<Vec<_>>();
    let gap = 8;
    let width = fields.iter().map(|field| field.width as usize * MAGNIFIED + gap).sum::<usize>() + gap;
    let height = fields.iter().map(|field| field.height as usize * MAGNIFIED).max().unwrap_or(0) + gap * 2;

    let mut rgb = vec![96; width * height * 3];
    let mut left = gap;
    for field in &fields {
        for y in 0..field.height as usize * MAGNIFIED {
            for x in 0..field.width as usize * MAGNIFIED {
                let from = ((y / MAGNIFIED) * field.width as usize + x / MAGNIFIED) * 3;
                let to = ((gap + y) * width + left + x) * 3;
                rgb[to..to + 3].copy_from_slice(&field.pixels[from..from + 3]);
            }
        }
        left += field.width as usize * MAGNIFIED + gap;
    }

    png(path, width, height, png::ColorType::Rgb, &rgb)
}

/// Draws [`TEXT`] at `size` pixels to the em from the fields, black on white: cut at 0.5 above, and smoothed over a
/// pixel below.
fn text(font: &Font, fields: &HashMap<GlyphId, Msdf>, size: f32) -> (usize, usize, Vec<u8>) {
    let shaped = font.shape(TEXT);
    let scale = font.pixels_per_unit(size);
    let factor = size / BASE_PIXELS_PER_EM;
    let margin = 8.0;
    let lines = shaped.lines.len() as f32;
    let widest = shaped.lines.iter().map(|line| line.width).max().unwrap_or(0) as f32 * scale;
    let block = lines * shaped.line_height as f32 * scale;
    let width = (widest + margin * 2.0).ceil() as usize;
    let height = (block * 2.0 + margin * 3.0).ceil() as usize;
    let mut grey = vec![255_u8; width * height];

    for (index, smooth) in [false, true].into_iter().enumerate() {
        let baseline = margin + index as f32 * (block + margin) + f32::from(font.line_metrics().ascender) * scale;
        for glyph in &shaped.glyphs {
            let Some(field) = fields.get(&glyph.glyph) else {
                continue;
            };
            let pen = glyph.pen(shaped.line_height, scale);
            let origin = (margin + pen.x, baseline - pen.y);
            draw(&mut grey, width, field, origin, factor, smooth);
        }
    }

    (width, height, grey)
}

/// Draws one glyph's field into `grey`, `width` pixels a row, its origin at `origin` in the picture's pixels, scaled by
/// `factor` from the base size.
fn draw(grey: &mut [u8], width: usize, field: &Msdf, origin: (f32, f32), factor: f32, smooth: bool) {
    let height = grey.len() / width;
    let left = origin.0 + field.left as f32 * factor;
    let top = origin.1 - field.top as f32 * factor;
    let columns = (left.floor().max(0.0) as usize)..((left + field.width as f32 * factor).ceil() as usize).min(width);
    let rows = (top.floor().max(0.0) as usize)..((top + field.height as f32 * factor).ceil() as usize).min(height);
    let range = field.range as f32 * 2.0 * factor;

    for y in rows {
        for x in columns.clone() {
            let texel_x = (x as f32 + 0.5 - left) / factor;
            let texel_y = (y as f32 + 0.5 - top) / factor;
            let median = field.median(texel_x, texel_y);
            let coverage = if smooth {
                ((median - 0.5) * range + 0.5).clamp(0.0, 1.0)
            } else if median > 0.5 {
                1.0
            } else {
                0.0
            };
            let ink = (255.0 * (1.0 - coverage)).round() as u8;
            let pixel = &mut grey[y * width + x];
            *pixel = (*pixel).min(ink);
        }
    }
}

/// `pixels` of `channels` bytes each, `width` a row, magnified `by` times, nearest: the new width, and the pixels.
fn magnify(width: usize, pixels: &[u8], channels: usize, by: usize) -> (usize, Vec<u8>) {
    let height = pixels.len() / (width * channels);
    let magnified = (0..height * by)
        .flat_map(|y| (0..width * by).map(move |x| (y / by * width + x / by) * channels))
        .flat_map(|from| pixels[from..from + channels].to_vec())
        .collect();
    (width * by, magnified)
}

/// Writes `pixels`, `width` by `height`, as a PNG at `path`.
fn png(path: &Path, width: usize, height: usize, colour: png::ColorType, pixels: &[u8]) -> Result<(), Box<dyn Error>> {
    let file = io::BufWriter::new(fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, u32::try_from(width)?, u32::try_from(height)?);
    encoder.set_color(colour);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    println!("wrote {}", path.display());
    Ok(())
}
