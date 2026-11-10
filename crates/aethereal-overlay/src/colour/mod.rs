//! The overlay's colours (ADR-0053: Colours): slate and earth. Healthy is one cool colour, and only warm colours mean
//! trouble. The table writes them in sRGB, and the renderer's colours are in linear light, so each is turned into
//! linear light as it is used.

#[cfg(test)]
mod tests;

use aethereal_data::Plain;
use aethereal_render::text::Colour;

/// The panels, drawn at [`PANEL_OPACITY`] so the program shows behind them: `#15181b`.
pub const SLATE: Swatch = Swatch::hex(0x15_18_1b);

/// Text: `#e4e0d6`.
pub const BONE: Swatch = Swatch::hex(0xe4_e0_d6);

/// Labels, the chart's budget line, a log line's step and actor, and an info line: `#8d918f`.
pub const STONE: Swatch = Swatch::hex(0x8d_91_8f);

/// Healthy lines and fills, a frame within its budget, and the command typed at the console: `#9dbfb0`.
pub const LICHEN: Swatch = Swatch::hex(0x9d_bf_b0);

/// Marks, on the chart and in the scrollback: `#d9b86c`.
pub const OCHRE: Swatch = Swatch::hex(0xd9_b8_6c);

/// A frame over its budget, and a warning: `#d08a4c`.
pub const RUST: Swatch = Swatch::hex(0xd0_8a_4c);

/// A frame over twice its budget, an error, and a refusal: `#cc5f52`.
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
