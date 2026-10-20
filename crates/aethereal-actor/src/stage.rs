//! The stages of a program's lifecycle, which the engine runs and sends its actors (ADR-0020).

use crate::Actors;
use aethereal_data::{Encode, Kind, KindName, Plain};

/// A stage of the program's lifecycle: a kind the engine sends every actor that subscribed to it, and waits, before the
/// next, until every turn it set off has run (ADR-0020).
///
/// The engine makes the stage's value itself, so a stage holds nothing it
/// cannot make: its default.
pub trait Stage: Kind + Encode<Actors> + Default + 'static {}

/// The first stage, sent once, when the host starts the engine: what every actor made by then subscribed to it is
/// sent before anything the program runs.
#[derive(Plain, Default)]
pub struct Start;

impl Kind for Start {
    const NAME: KindName = KindName::new("aether.lifecycle.start");
}

impl Stage for Start {}

/// The last stage, sent once, when the program is to stop: once every turn it set off has run, the host drops the
/// engine.
#[derive(Plain, Default)]
pub struct Stop;

impl Kind for Stop {
    const NAME: KindName = KindName::new("aether.lifecycle.stop");
}

impl Stage for Stop {}
