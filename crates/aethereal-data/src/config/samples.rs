//! Sections the tests of this module compose.

use crate::Plain;
use alloc::string::String;
use alloc::vec::Vec;

/// The engine's section, with a flag of each kind of suggestion and a field with none.
#[derive(Plain, Debug, PartialEq, Eq)]
#[kind(name = "test.config.engine")]
#[section(name = "engine")]
pub struct EngineConfig {
    /// The threads that run a step.
    #[flag(long = "workers", short = 'w')]
    pub workers: u32,
    /// The bytes one piece of mail may hold.
    pub mail_bytes: u64,
    /// Whether turns are fused.
    #[flag]
    pub fused: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { workers: 1, mail_bytes: 1 << 20, fused: true }
    }
}

/// A section that also declares the name `engine`.
#[derive(Plain, Debug, Default, PartialEq, Eq)]
#[kind(name = "test.config.other_engine")]
#[section(name = "engine")]
pub struct OtherEngine {
    pub threads: u32,
}

/// A section whose part suggests the flag the engine's does.
#[derive(Plain, Debug, Default, PartialEq, Eq)]
#[kind(name = "test.config.pool")]
#[section(name = "pool")]
pub struct PoolConfig {
    /// The threads of the pool.
    #[flag(long = "workers")]
    pub workers: u32,
}

/// One window.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub title: String,
    pub size: [u32; 2],
}

/// The windows' section: a sequence, written as the sequence alone.
#[derive(Plain, Debug, PartialEq, Eq)]
#[kind(name = "test.config.windows")]
#[section(name = "windows")]
pub struct WindowsConfig(pub Vec<Window>);

impl Default for WindowsConfig {
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

/// The renderer's section.
#[derive(Plain, Debug, Default, PartialEq, Eq)]
#[kind(name = "test.config.renderer")]
#[section(name = "renderer")]
pub struct RendererConfig {
    /// How frames are presented.
    #[flag]
    pub presentation: Presentation,
}
