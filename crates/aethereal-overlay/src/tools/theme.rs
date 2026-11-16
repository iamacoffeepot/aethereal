//! The `set_theme` tool (ADR-0053: Colours): it names the theme the overlay's greys are tinted by, and the next frame is
//! drawn in it; named nothing, it lists the themes and says which is in use. The overlay's tools' worker reads the call
//! and asks the overlay, whose state the theme is, for the [`Choice`] it makes, and answers what the overlay answered. It
//! is offered in every engine, trusted or not, and called by anyone: it changes nothing but how this engine's overlay
//! looks. The choice is the engine's own, as the console's height is, and is not kept past the engine's end.

use crate::colour::{Palette, Theme};
use aethereal_data::{Plain, Schema, SchemaNodes};
use aethereal_tools::{Output, ToolSheet};

/// The tool that names the overlay's theme.
pub const SET_THEME: &str = "set_theme";

/// Tints the overlay's greys by `theme`, or with none named lists the themes.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.choose_theme")]
pub struct ChooseTheme {
    /// The theme: `lichen`, lichen grey, the first; `slate`, blue-grey; or `stone`, warm stone. Left out, the themes
    /// are listed and nothing changes. An `Option` only because the command line reads a flag that may be left out as
    /// one; it is read once, as a [`Choice`], by [`Self::choice`].
    #[flag(position)]
    theme: Option<Theme>,
}

/// What a call of `set_theme` asks the overlay for: the themes listed, the palette kept; or the palette switched to a
/// theme. Answered with [`ThemesShown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.theme_choice")]
pub enum Choice {
    List,
    Switch(Theme),
}

impl ChooseTheme {
    /// The input that names `theme`, as a caller writes it.
    #[cfg(test)]
    pub const fn naming(theme: Theme) -> Self {
        Self { theme: Some(theme) }
    }

    /// What it asks for.
    pub fn choice(self) -> Choice {
        self.theme.map_or(Choice::List, Choice::Switch)
    }
}

/// The themes, and the one the overlay is drawn in now: the overlay's answer to a [`Choice`], and what `set_theme`
/// answers.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.themes_shown")]
pub struct ThemesShown {
    pub current: String,
    pub themes: Vec<String>,
}

impl ThemesShown {
    /// Every theme, and `current`.
    fn of(current: Theme) -> Self {
        Self {
            current: String::from(current.name()),
            themes: Theme::ALL.map(|theme| String::from(theme.name())).into(),
        }
    }
}

/// The sheet of `set_theme`.
pub fn theme_sheet() -> ToolSheet {
    let theme = "Tints the dev overlay's greys by the theme named, `lichen` (lichen grey, the first), `slate`, or \
                 `stone` (warm stone), and the overlay's next frame is drawn in it; the accents, healthy lichen and the \
                 warm colours of trouble, are the same in every theme. Named nothing, it changes nothing. Either way \
                 it answers the themes and the one in use. The choice is this engine's alone and lasts until it ends.";

    ToolSheet::of::<ChooseTheme>(SET_THEME, theme, Output::Value(SchemaNodes::of(&ThemesShown::SHAPE)))
}

/// `palette` after `choice`: a new one of the theme it switches to, or the same when it lists them; and what the tool
/// answers.
pub fn chosen(palette: &Palette, choice: Choice) -> (Palette, ThemesShown) {
    let palette = match choice {
        Choice::Switch(theme) => Palette::new(theme),
        Choice::List => *palette,
    };

    (palette, ThemesShown::of(palette.theme()))
}
