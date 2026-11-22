//! Who hears where the repository stands (ADR-0057, ADR-0059): an actor that would know whether the repository is
//! served asks `Git` to [`HearReadiness`], and is told its [`Readiness`] at once and again each time it changes, so
//! `Git` names nothing that listens to it, and a program that runs no listener has none.
//!
//! ```text
//! HearReadiness ──> Git: the sender held, and told where the repository stands ──> Readiness
//! the repository served, or found never to be ──> Readiness ──> each actor that hears
//! ```

use super::{Git, Readiness};
use aethereal_actor::{Fault, Speaks, Turn};
use aethereal_data::Plain;

/// What an actor that hears where the repository stands is told.
pub type Hearing = (Readiness,);

/// Asks `Git` to tell the sender where the repository stands: at once, and again each time it changes. The sender must
/// handle each kind of [`Hearing`], and is let go as it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.hear_readiness")]
pub struct HearReadiness;

impl Git {
    /// Holds the sender, and tells it where the repository stands: nothing, when it hears already, as it was told then
    /// and is told each change after.
    pub(super) fn handle_hear_readiness(&mut self, turn: &mut Turn<'_, Self, Speaks<Hearing>>) -> Result<(), Fault> {
        let hearer = *turn.from();
        let added = self.hearers.subscribe(turn, hearer);
        if !added {
            return Ok(());
        }

        turn.to(hearer).send(&self.repository.readiness())?;

        Ok(())
    }
}
