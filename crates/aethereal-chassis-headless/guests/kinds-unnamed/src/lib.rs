//! Spike: a crate the echo guest depends on and never names (ADR-0054).

use aethereal_data::Plain;

/// In a crate nothing names.
#[derive(Plain)]
#[kind(name = "spike.unnamed.alone")]
pub struct Alone {
    pub n: u64,
}
