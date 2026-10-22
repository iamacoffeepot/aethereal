//! A block font, five blocks square, for the score and the board's words: each glyph is drawn as the runs of blocks
//! along each of its rows, a rectangle each.

use super::records::{Colour, Plane, Records};

/// The blocks a glyph is wide and tall.
const BLOCKS: u8 = 5;

/// The rows of the glyph for `letter`, top first, a block where each holds `X`: nothing for a space, or for a letter
/// the font does not hold.
const fn glyph(letter: char) -> [&'static str; 5] {
    match letter {
        '0' => ["XXXXX", "X...X", "X...X", "X...X", "XXXXX"],
        '1' => [".XX..", "..X..", "..X..", "..X..", ".XXX."],
        '2' => ["XXXXX", "....X", "XXXXX", "X....", "XXXXX"],
        '3' => ["XXXXX", "....X", ".XXXX", "....X", "XXXXX"],
        '4' => ["X...X", "X...X", "XXXXX", "....X", "....X"],
        '5' => ["XXXXX", "X....", "XXXXX", "....X", "XXXXX"],
        '6' => ["XXXXX", "X....", "XXXXX", "X...X", "XXXXX"],
        '7' => ["XXXXX", "....X", "...X.", "..X..", "..X.."],
        '8' => ["XXXXX", "X...X", "XXXXX", "X...X", "XXXXX"],
        '9' => ["XXXXX", "X...X", "XXXXX", "....X", "XXXXX"],
        'A' => [".XXX.", "X...X", "XXXXX", "X...X", "X...X"],
        'C' => [".XXXX", "X....", "X....", "X....", ".XXXX"],
        'E' => ["XXXXX", "X....", "XXXX.", "X....", "XXXXX"],
        'G' => [".XXXX", "X....", "X..XX", "X...X", ".XXX."],
        'I' => [".XXX.", "..X..", "..X..", "..X..", ".XXX."],
        'L' => ["X....", "X....", "X....", "X....", "XXXXX"],
        'N' => ["X...X", "XX..X", "X.X.X", "X..XX", "X...X"],
        'O' => [".XXX.", "X...X", "X...X", "X...X", ".XXX."],
        'P' => ["XXXX.", "X...X", "XXXX.", "X....", "X...."],
        'R' => ["XXXX.", "X...X", "XXXX.", "X..X.", "X...X"],
        'S' => [".XXXX", "X....", ".XXX.", "....X", "XXXX."],
        'T' => ["XXXXX", "..X..", "..X..", "..X..", "..X.."],
        'U' => ["X...X", "X...X", "X...X", "X...X", ".XXX."],
        'W' => ["X...X", "X...X", "X.X.X", "XX.XX", "X...X"],
        'Y' => ["X...X", ".X.X.", "..X..", "..X..", "..X.."],
        _ => [".....", ".....", ".....", ".....", "....."],
    }
}

/// Where a line of text stands: centred across at `x`, its top at `top`, each block `block` court units square.
#[derive(Clone, Copy)]
pub struct Line {
    pub x: f32,
    pub top: f32,
    pub block: f32,
}

/// Writes `text` into `records` in `plane`, coloured `colour`, where `line` says: each glyph five blocks wide with a
/// block between glyphs.
pub fn write(records: &mut Records, text: &str, line: Line, plane: Plane, colour: Colour) {
    let advance = f32::from(BLOCKS + 1) * line.block;
    let glyphs = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
    let width = f32::from(glyphs).mul_add(advance, -line.block);
    let left = width.mul_add(-0.5, line.x);
    for (at, letter) in (0_u16..).zip(text.chars()) {
        let glyph_left = f32::from(at).mul_add(advance, left);
        for (row, blocks) in (0_u8..).zip(glyph(letter)) {
            let top = f32::from(row).mul_add(-line.block, line.top);
            for (first, count) in runs(blocks) {
                let half_width = f32::from(count) * line.block * 0.5;
                let x = f32::from(first).mul_add(line.block, glyph_left) + half_width;
                records.rect(x, line.block.mul_add(-0.5, top), half_width, line.block * 0.5, plane, colour);
            }
        }
    }
}

/// The runs of blocks along a glyph's row: where each begins, and how many blocks it holds.
fn runs(row: &str) -> Vec<(u8, u8)> {
    let mut runs: Vec<(u8, u8)> = Vec::new();
    let mut previous = b'.';
    for (at, cell) in (0_u8..).zip(row.bytes()) {
        let filled = cell == b'X';
        let was_filled = previous == b'X';
        let begins = filled && !was_filled;
        let goes_on = filled && was_filled;
        if begins {
            runs.push((at, 1));
        }
        if goes_on && let Some((_, count)) = runs.last_mut() {
            *count += 1;
        }
        previous = cell;
    }

    runs
}
