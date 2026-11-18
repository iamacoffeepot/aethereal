//! Who hears of the engines the hub trusts (ADR-0057): an actor that takes what trusted engines say asks `Engines`
//! to [`HearTrusted`], and is told each such engine as a reference, so `Engines` and its engines depend on nothing that
//! listens to them, and a hub that runs no listener has none.
//!
//! ```text
//! HearTrusted ──> Engines: the sender held, and told each engine linked trusted already ──> TrustedEngine { engine }
//! an engine's token taken, its trust the owner's ──> TrustedEngine { engine } ──> each actor that hears
//! ```

use super::{Engine, Engines};
use aethereal_actor::{ActorRef, Anyone, Fault, Handles, Level, Turn};
use aethereal_data::{Bound, Plain};

/// What an actor that hears of trusted engines is told.
pub type Hearing = (TrustedEngine,);

/// Asks `Engines` to tell the sender each engine the hub trusts: every one linked trusted already, at once, and each
/// that links trusted after, as its token is taken. The sender must handle each kind of [`Hearing`], and is let go as
/// it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.hear_trusted")]
pub struct HearTrusted;

/// An engine the hub trusts, its token taken: its actor, to reach it by.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.trusted_engine")]
pub struct TrustedEngine {
    pub engine: ActorRef<Engine>,
}

impl Handles<HearTrusted> for Engines {
    type From = Anyone;
    type Returns = ();

    /// Holds the sender, and tells it each engine linked trusted already: nothing, when it hears already, as it was
    /// told them then and is told each after.
    fn handle(&mut self, _: HearTrusted, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(hearer) = turn.cast::<Hearing>(sender) else {
            turn.log(Level::Warn, format_args!("{sender:?}, which takes no trusted engine, asked to hear of them"));
            return Ok(());
        };
        let added = self.hearers.subscribe(turn, hearer);
        if !added {
            return Ok(());
        }
        for engine in self.trusted() {
            turn.to(hearer).send(&TrustedEngine { engine })?;
        }

        Ok(())
    }
}
