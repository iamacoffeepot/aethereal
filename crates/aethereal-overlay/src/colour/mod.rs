//! The overlay's colours (ADR-0053: Colours): slate and earth. Healthy is one cool colour, and only warm colours mean
//! trouble; the frame chart's three bars are told apart by colour alone, the same whatever the budget, as their height
//! against the budget's line says over or under, and the GPU graph's passes by colours of their own. The table writes
//! them in sRGB, and the renderer's colours are in linear light, so each is turned into linear light as it is used.

#[cfg(test)]
mod tests;

use aethereal_data::Plain;
use aethereal_render::text::Colour;

/// The band's panel, drawn at [`PANEL_OPACITY`] so the program shows behind it: `#15181b`.
pub const SLATE: Swatch = Swatch::hex(0x15_18_1b);

/// The console's scrollback, a step lighter than the band's slate, about five points of lightness, so the two panels
/// read apart: `#20252a`.
pub const SLATE_LIGHTER: Swatch = Swatch::hex(0x20_25_2a);

/// The prompt's strip, a step darker than the band's slate, a well the line is typed into: `#111417`.
pub const SLATE_DARKER: Swatch = Swatch::hex(0x11_14_17);

/// Text, and the chart's dashed budget line: `#e4e0d6`.
pub const BONE: Swatch = Swatch::hex(0xe4_e0_d6);

/// Labels, the chart's faint line at twice the budget, a log line's step and actor, and an info line: `#8d918f`.
pub const STONE: Swatch = Swatch::hex(0x8d_91_8f);

/// Healthy lines and fills, the chart's median bar, the middle of a column's three, and the command typed at the
/// console: `#9dbfb0`.
pub const LICHEN: Swatch = Swatch::hex(0x9d_bf_b0);

/// The chart's slowest bar, the tallest of a column's three, at the back: `#6b5f4c`.
pub const SAND: Swatch = Swatch::hex(0x6b_5f_4c);

/// The chart's fastest bar, the shortest of a column's three, in front: `#3f5a57`.
pub const SLATE_TEAL: Swatch = Swatch::hex(0x3f_5a_57);

/// The GPU graph's first pass in each three, and its figure: `#6f8aa6`.
pub const SLATE_BLUE: Swatch = Swatch::hex(0x6f_8a_a6);

/// The GPU graph's second pass in each three, and its figure: `#8a7ca8`.
pub const MUTED_VIOLET: Swatch = Swatch::hex(0x8a_7c_a8);

/// The GPU graph's third pass in each three, and its figure: `#a88aa0`.
pub const MAUVE: Swatch = Swatch::hex(0xa8_8a_a0);

/// The GPU graph's passes, by their labels in the order they were first heard, cycling: cool and muted, and none of
/// the frame chart's three, so lichen, sand, and slate-teal keep their one meaning each.
pub const PASS_SWATCHES: [Swatch; 3] = [SLATE_BLUE, MUTED_VIOLET, MAUVE];

/// Marks, on the chart and in the scrollback: `#d9b86c`.
pub const OCHRE: Swatch = Swatch::hex(0xd9_b8_6c);

/// A warning, and the console's count of them: `#d08a4c`.
pub const RUST: Swatch = Swatch::hex(0xd0_8a_4c);

/// An error, the console's count of them, and a refusal: `#cc5f52`.
pub const BRICK: Swatch = Swatch::hex(0xcc_5f_52);

/// How opaque the panels are drawn: 85%, so the program shows behind them.
pub const PANEL_OPACITY: f32 = 0.85;

/// A colour as the ADR's table writes it: sRGB, eight bits a channel.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swatch {
    red: u8,
    green: u8,
    blue: u8,
}

impl Swatch {
    /// The colour written `0xRRGGBB`.
    const fn hex(rgb: u32) -> Self {
        let [_, red, green, blue] = rgb.to_be_bytes();

        Self { red, green, blue }
    }

    /// The colour in linear light, opaque.
    #[must_use]
    pub fn opaque(self) -> Colour {
        self.at(1.0)
    }

    /// The colour in linear light, at `alpha`.
    #[must_use]
    pub fn at(self, alpha: f32) -> Colour {
        Colour { red: linear(self.red), green: linear(self.green), blue: linear(self.blue), alpha }
    }
}

/// One sRGB channel in linear light, by the sRGB transfer function: a straight line near black, a curve above it.
fn linear(channel: u8) -> f32 {
    let encoded = f32::from(channel) / 255.0;
    if encoded <= 0.040_45 {
        return encoded / 12.92;
    }

    ((encoded + 0.055) / 1.055).powf(2.4)
}
