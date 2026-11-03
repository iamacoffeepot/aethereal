//! The buffers of freed blobs, handed back to the actors that put them, and the buffers jobs were lent and did not
//! take, given back to the actors that ran them, to put in again (ADR-0042).

#[cfg(test)]
mod tests;

use super::{Held, part};
use crate::BySeat;
use aethereal_actor::{Keeping, Maker, Seated, Spares};

/// Hands `buffer`, freed from a blob `maker` put, back to that actor while it sits in `actors`, the engine's parts,
/// to keep up to `most_kept_bytes`; drops it otherwise: the chassis keeps none, and an actor that has gone keeps
/// nothing more. The engine calls it as a step's blobs are swept, with every part back in it, so no worker holds an
/// actor it reaches.
pub(super) fn hand_back(actors: &mut [BySeat<Held>], buffer: Vec<u8>, maker: Maker, most_kept_bytes: usize) {
    let Maker::Actor(by) = maker else {
        return;
    };
    if let Some(keeping) = keeping(actors, by) {
        keeping.keep_buffer(buffer, most_kept_bytes);
    }
}

/// Gives `lent`, the buffers a job the actor `by` ran was lent and did not take, back to that actor while it sits in
/// `actors`, to keep up to `most_kept_bytes`; drops them otherwise. The engine calls it between steps, as it answers
/// the job or drops it before it was handed out, with every part in it.
pub(super) fn give_back(actors: &mut [BySeat<Held>], by: Seated, lent: Spares, most_kept_bytes: usize) {
    // Most jobs are lent nothing, or take what they are lent.
    if lent.is_empty() {
        return;
    }
    if let Some(keeping) = keeping(actors, by) {
        keeping.keep_buffers(lent, most_kept_bytes);
    }
}

/// What the actor `by` keeps, while it sits in `actors`.
fn keeping(actors: &mut [BySeat<Held>], by: Seated) -> Option<&mut Keeping> {
    let parts = actors.len();

    actors[part(by.seat, parts)].get_mut(by).map(|held| &mut held.keeping)
}
