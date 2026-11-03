//! The buffers of freed blobs, handed back to the actors that put them, to put in again (ADR-0042).

#[cfg(test)]
mod tests;

use super::{Held, part};
use crate::BySeat;
use aethereal_actor::Maker;

/// Hands `buffer`, freed from a blob `maker` put, back to that actor while it sits in `actors`, the engine's parts,
/// to keep up to `most_kept_bytes`; drops it otherwise: the chassis keeps none, and an actor that has gone keeps
/// nothing more. The engine calls it as a step's blobs are swept, with every part back in it, so no worker holds an
/// actor it reaches.
pub(super) fn hand_back(actors: &mut [BySeat<Held>], buffer: Vec<u8>, maker: Maker, most_kept_bytes: usize) {
    let Maker::Actor(by) = maker else {
        return;
    };
    let parts = actors.len();
    if let Some(held) = actors[part(by.seat, parts)].get_mut(by) {
        held.keeping.keep_buffer(buffer, most_kept_bytes);
    }
}
