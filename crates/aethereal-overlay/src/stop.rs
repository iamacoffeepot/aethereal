//! The overlay's three stops, which the backtick steps through (ADR-0053: The three stops).

use aethereal_data::Plain;

/// Where the overlay stands: hidden, drawing nothing; the band across the top of the window; or the band with the
/// console beneath it. The backtick steps it hidden, band, console, and hidden again.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Hidden,
    Band,
    Console,
}

impl Stop {
    /// The stop the backtick steps to from this one.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Hidden => Self::Band,
            Self::Band => Self::Console,
            Self::Console => Self::Hidden,
        }
    }

    /// Whether the overlay draws anything at this stop.
    #[must_use]
    pub const fn draws(self) -> bool {
        match self {
            Self::Hidden => false,
            Self::Band | Self::Console => true,
        }
    }

    /// Whether the console's panel is drawn beneath the band at this stop.
    #[must_use]
    pub const fn console(self) -> bool {
        match self {
            Self::Hidden | Self::Band => false,
            Self::Console => true,
        }
    }
}
