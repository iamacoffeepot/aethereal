//! Settings kinds the tests of this module compose.

use crate::Plain;
use alloc::string::String;
use alloc::vec::Vec;

/// The engine's settings, with a flag of each kind of suggestion and a field with none.
#[derive(Plain, Debug, PartialEq, Eq)]
#[kind(name = "test.config.engine")]
#[settings]
pub struct EngineSettings {
    /// The threads that run a step.
    #[flag(long = "workers", short = 'w')]
    pub workers: u32,
    /// The bytes one piece of mail may hold.
    pub mail_bytes: u64,
    /// Whether turns are fused.
    #[flag]
    pub fused: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self { workers: 1, mail_bytes: 1 << 20, fused: true }
    }
}

/// Settings that suggest the flag the engine's do.
#[derive(Plain, Debug, Default, PartialEq, Eq)]
#[kind(name = "test.config.pool")]
#[settings]
pub struct PoolSettings {
    /// The threads of the pool.
    #[flag(long = "workers")]
    pub workers: u32,
}

/// One window: an element of the windows' settings, with a handle for each field.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "test.config.window")]
#[settings]
pub struct Window {
    /// The window's title.
    pub title: String,
    /// The window's inner size.
    pub size: [u32; 2],
}

/// The windows' settings: a sequence, written as the sequence alone.
#[derive(Plain, Debug, PartialEq, Eq)]
#[kind(name = "test.config.windows")]
#[settings]
pub struct WindowsSettings(pub Vec<Window>);

impl Default for WindowsSettings {
    fn default() -> Self {
        Self(Vec::from([Window { title: String::from("aethereal"), size: [800, 600] }]))
    }
}

/// How frames are presented.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    /// One frame each refresh.
    #[default]
    Fifo,
    /// As fast as frames are made.
    VeryFast,
}

/// How sharply a canvas is drawn: variants of no field, one positional field, one named field, and two fields.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
pub enum Density {
    /// Every device pixel.
    Screen,
    /// At most this many pixels to each CSS pixel.
    AtMost(f64),
    /// At least this many.
    AtLeast { pixels: u32 },
    /// Between two.
    Between(f64, f64),
}

/// The renderer's settings.
#[derive(Plain, Debug, Default, PartialEq, Eq)]
#[kind(name = "test.config.renderer")]
#[settings]
pub struct RendererSettings {
    /// How frames are presented.
    #[flag]
    pub presentation: Presentation,
}
