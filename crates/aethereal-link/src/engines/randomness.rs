//! The job that draws a token from the host's randomness, which `Engines` mints its tokens from (ADR-0034, ADR-0040).

use crate::Token;
use aethereal_actor::{Fault, Job, Randomness, Runner, Running, Unfilled};
use aethereal_data::Plain;

/// Draws a token's bytes from the host's randomness (ADR-0018: `Running::service`). It is private to this crate, so
/// only `Engines` mints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.draw")]
pub struct Draw;

/// What a draw came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.drawn")]
pub enum Drawn {
    /// A token, of the host's randomness.
    Token(Token),
    /// The host's randomness filled nothing.
    Unfilled,
    /// The host serves no randomness.
    Unavailable,
}

/// It asks the host's randomness, which answers at once.
impl Job for Draw {
    type Done = Drawn;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Drawn, Fault> {
        let Some(randomness) = job.service::<dyn Randomness>() else {
            return Ok(Drawn::Unavailable);
        };
        let mut bytes = [0; 16];
        let drawn = match randomness.fill(&mut bytes) {
            Ok(()) => Drawn::Token(Token(bytes)),
            Err(Unfilled) => Drawn::Unfilled,
        };

        Ok(drawn)
    }
}
