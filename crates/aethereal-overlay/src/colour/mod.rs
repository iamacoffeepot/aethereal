//! The overlay's colours (ADR-0053: Colours): one ladder of greys at one hue, its surfaces and its inks, tinted by the
//! theme, and accents that keep their meaning whatever the theme. Healthy is one cool colour, and only warm colours mean
//! trouble; the frame chart's three bars are told apart by colour alone, the same whatever the budget, as their height
//! against the budget's line says over or under, and the GPU graph's passes by colours of their own. A widget asks for
//! a colour by its role, a [`Surface`] or an [`Ink`], and the overlay's [`Palette`] answers it.
//!
//! ```text
//! Theme ──> hue, surface chroma, ink chroma ──> each role's lightness, chroma, hue (OKLCH) ──> sRGB, eight bits a
//!           channel, as the owner's palette sheet shows it ──> linear light, as the renderer draws ──> Palette
//! ```
//!
//! The palette is worked out once, when the overlay begins and when its theme changes, never as a frame is drawn.

#[cfg(test)]
mod tests;

use aethereal_data::Plain;
use aethereal_render::text::{Colour, Run};

/// How the greys are tinted: their hue, and the chroma of the surfaces and of the inks. The accents are the same in
/// every theme. Each is named by its word, as the `set_theme` tool takes it.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[kind(name = "aether.overlay.theme")]
pub enum Theme {
    /// Lichen grey: greys leaning toward the healthy colour, so the neutrals and lichen read as one family, at hue 185.
    #[default]
    Lichen,
    /// Slate: blue-grey, at hue 250.
    Slate,
    /// Warm stone: greys toward paper and earth, at hue 80.
    Stone,
}

impl Theme {
    /// Every theme, in the order `set_theme` lists them.
    pub const ALL: [Self; 3] = [Self::Lichen, Self::Slate, Self::Stone];

    /// The word the `set_theme` tool names it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lichen => "lichen",
            Self::Slate => "slate",
            Self::Stone => "stone",
        }
    }

    /// Its hue, in degrees.
    const fn hue(self) -> f64 {
        match self {
            Self::Lichen => 185.0,
            Self::Slate => 250.0,
            Self::Stone => 80.0,
        }
    }

    /// The chroma of its surfaces.
    const fn surface_chroma(self) -> f64 {
        match self {
            Self::Lichen => 0.011,
            Self::Slate => 0.012,
            Self::Stone => 0.010,
        }
    }

    /// The chroma of its neutral inks.
    const fn ink_chroma(self) -> f64 {
        match self {
            Self::Lichen | Self::Slate => 0.010,
            Self::Stone => 0.014,
        }
    }
}

/// What a panel is drawn on: the ladder's surfaces, darkest first, each a fixed step of lightness from the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The tabs' strip.
    Deepest,
    /// A closed tab with the pointer over it: halfway from the strip to the open tab.
    Hover,
    /// The prompt's well, the grip's well, and the open tab.
    Inset,
    /// The band's panel, drawn at 85% so the program shows behind it.
    Band,
    /// The transcript and the scrollback.
    Sheet,
    /// The owner's own lines.
    Raised,
}

impl Surface {
    /// Every surface, in the order the palette holds them.
    const ALL: [Self; 6] = [Self::Deepest, Self::Hover, Self::Inset, Self::Band, Self::Sheet, Self::Raised];

    /// Its lightness.
    const fn lightness(self) -> f64 {
        match self {
            Self::Deepest => 0.160,
            Self::Hover => 0.174,
            Self::Inset => 0.188,
            Self::Band => 0.207,
            Self::Sheet => 0.255,
            Self::Raised => 0.300,
        }
    }

    /// How opaque it is drawn: the band's panel at 85%, so the program shows behind it; the console's panels at 96%,
    /// nearly solid, so the console reads as a sheet laid over the scene and the band's see-through graphs above it
    /// never read as part of its text.
    const fn opacity(self) -> f32 {
        match self {
            Self::Band => 0.85,
            Self::Deepest | Self::Hover | Self::Inset | Self::Sheet | Self::Raised => 0.96,
        }
    }
}

/// What a line, a figure, a bar, or a text is drawn in: the ladder's inks, tinted by the theme, then the accents, the
/// same in every theme.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Every rule and divider, the grip's shadow aside.
    Divider,
    /// Hints: the completion shown ahead of the line.
    Faint,
    /// Labels, tool output, info lines, and a tab not open.
    Dim,
    /// Text, Claude's replies, and the chart's budget line.
    Text,
    /// The GPU graph's first pass in each three, and its figure. The three passes share one lightness and chroma, so
    /// they are told apart by hue alone.
    Pass1,
    /// The GPU graph's second pass in each three, and its figure.
    Pass2,
    /// The GPU graph's third pass in each three, and its figure.
    Pass3,
    /// The chart's fastest bar, the shortest of a column's three, in front.
    Fast,
    /// The chart's median bar, the middle of a column's three.
    Median,
    /// The chart's slowest bar, the tallest, at the back.
    Slow,
    /// Healthy lines and meters, and the owner's sent lines.
    Lichen,
    /// The line being typed: a brighter lichen, so it reads apart from lines already sent.
    Input,
    /// Labels that name: who typed an owner's line.
    Label,
    /// A link in Claude's reply.
    Link,
    /// Marks, a session waiting on the owner, an ask, and a compaction.
    Mark,
    /// A warning, and a meter past 75%.
    Warn,
    /// An error, a refusal, and a meter past 90%.
    Error,
}

/// The GPU graph's passes, by their labels in the order they were first heard, cycling.
pub const PASSES: [Ink; 3] = [Ink::Pass1, Ink::Pass2, Ink::Pass3];

impl Ink {
    /// Every ink, in the order the palette holds them.
    const ALL: [Self; 17] = [
        Self::Divider,
        Self::Faint,
        Self::Dim,
        Self::Text,
        Self::Pass1,
        Self::Pass2,
        Self::Pass3,
        Self::Fast,
        Self::Median,
        Self::Slow,
        Self::Lichen,
        Self::Input,
        Self::Label,
        Self::Link,
        Self::Mark,
        Self::Warn,
        Self::Error,
    ];

    /// Its colour in `theme`: a neutral's lightness at the theme's hue and ink chroma, or an accent's own.
    fn oklch(self, theme: Theme) -> Oklch {
        let neutral = |lightness| Oklch { lightness, chroma: theme.ink_chroma(), hue: theme.hue() };
        match self {
            Self::Divider => neutral(0.360),
            Self::Faint => neutral(0.520),
            Self::Dim => neutral(0.680),
            Self::Text => neutral(0.910),
            Self::Pass1 | Self::Label => Oklch::of(0.650, 0.060, 250.0),
            Self::Pass2 => Oklch::of(0.650, 0.060, 300.0),
            Self::Pass3 => Oklch::of(0.650, 0.060, 340.0),
            Self::Fast => Oklch::of(0.450, 0.035, 188.0),
            Self::Median | Self::Lichen => Oklch::of(0.775, 0.045, 167.0),
            Self::Slow => Oklch::of(0.500, 0.035, 80.0),
            Self::Input => Oklch::of(0.880, 0.055, 167.0),
            Self::Link => Oklch::of(0.740, 0.070, 245.0),
            Self::Mark => Oklch::of(0.800, 0.100, 87.0),
            Self::Warn => Oklch::of(0.700, 0.115, 60.0),
            Self::Error => Oklch::of(0.630, 0.140, 29.0),
        }
    }
}

/// Every colour of one theme, in linear light, worked out once.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    theme: Theme,
    surfaces: [Colour; 6],
    inks: [Colour; 17],
}

impl Default for Palette {
    fn default() -> Self {
        Self::new(Theme::default())
    }
}

impl Palette {
    /// The palette of `theme`.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        let surface = |surface: Surface| {
            Oklch { lightness: surface.lightness(), chroma: theme.surface_chroma(), hue: theme.hue() }
                .colour(surface.opacity())
        };

        Self { theme, surfaces: Surface::ALL.map(surface), inks: Ink::ALL.map(|ink| ink.oklch(theme).colour(1.0)) }
    }

    /// The theme it is of.
    #[must_use]
    pub const fn theme(&self) -> Theme {
        self.theme
    }

    /// `surface`, at its opacity.
    #[must_use]
    pub const fn surface(&self, surface: Surface) -> Colour {
        self.surfaces[surface as usize]
    }

    /// `ink`, opaque.
    #[must_use]
    pub const fn ink(&self, ink: Ink) -> Colour {
        self.inks[ink as usize]
    }

    /// `ink`, at `alpha`.
    #[must_use]
    pub const fn faded(&self, ink: Ink, alpha: f32) -> Colour {
        Colour { alpha, ..self.ink(ink) }
    }

    /// A run of `text` in `ink`.
    #[must_use]
    pub fn run(&self, text: &str, ink: Ink) -> Run {
        Run::new(text, self.ink(ink))
    }
}

/// A colour in OKLCH: its lightness, its chroma, and its hue in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Oklch {
    lightness: f64,
    chroma: f64,
    hue: f64,
}

impl Oklch {
    const fn of(lightness: f64, chroma: f64, hue: f64) -> Self {
        Self { lightness, chroma, hue }
    }

    /// In sRGB, eight bits a channel, as the owner's palette sheet writes it: by way of `OKLab` and linear light, each
    /// channel held to the gamut.
    fn srgb(self) -> [u8; 3] {
        let (sine, cosine) = self.hue.to_radians().sin_cos();
        let (a, b) = (self.chroma * cosine, self.chroma * sine);
        let long = 0.396_337_777_4f64.mul_add(a, 0.215_803_757_3f64.mul_add(b, self.lightness)).powi(3);
        let middle = (-0.105_561_345_8f64).mul_add(a, (-0.063_854_172_8f64).mul_add(b, self.lightness)).powi(3);
        let short = (-0.089_484_177_5f64).mul_add(a, (-1.291_485_548_0f64).mul_add(b, self.lightness)).powi(3);
        let linear = [
            4.076_741_662_1f64.mul_add(long, (-3.307_711_591_3f64).mul_add(middle, 0.230_969_929_2 * short)),
            (-1.268_438_004_6f64).mul_add(long, 2.609_757_401_1f64.mul_add(middle, -0.341_319_396_5 * short)),
            (-0.004_196_086_3f64).mul_add(long, (-0.703_418_614_7f64).mul_add(middle, 1.707_614_701_0 * short)),
        ];

        linear.map(encoded)
    }

    /// In linear light, at `alpha`.
    fn colour(self, alpha: f32) -> Colour {
        let [red, green, blue] = self.srgb().map(linear);

        Colour { red, green, blue, alpha }
    }
}

/// One channel in linear light, held to the gamut, as sRGB writes it in eight bits: a straight line near black, a
/// curve above it.
fn encoded(channel: f64) -> u8 {
    let channel = channel.clamp(0.0, 1.0);
    let curved = if channel <= 0.003_130_8 {
        12.92 * channel
    } else {
        1.055f64.mul_add(channel.powf(1.0 / 2.4), -0.055)
    };
    // A share of the gamut from nothing to one, scaled to eight bits, which a u8 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (curved.clamp(0.0, 1.0) * 255.0).round() as u8;

    byte
}

/// One sRGB channel in linear light, by the sRGB transfer function: a straight line near black, a curve above it.
fn linear(channel: u8) -> f32 {
    let encoded = f32::from(channel) / 255.0;
    if encoded <= 0.040_45 {
        return encoded / 12.92;
    }

    ((encoded + 0.055) / 1.055).powf(2.4)
}
