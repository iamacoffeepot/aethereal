//! The host's randomness, which `Engines` mints its tokens from: a service the host serves its jobs, and the job that
//! draws a token through it (ADR-0034, ADR-0040).

use crate::Token;
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Why the host's randomness filled nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unfilled;

/// The host's randomness (ADR-0040): bytes no other process can guess, which `Engines` mints its tokens from. A host
/// serves it into the services its runners hand every job (ADR-0034), as it bootstraps `Engines`; a host that cannot
/// serve it does not bootstrap `Engines`.
///
/// A fill never blocks the thread that polls it: a host's randomness is ready once its machine has started.
pub trait Randomness: Send + Sync {
    /// Fills `bytes` with the host's randomness.
    fn fill(&self, bytes: &mut [u8]) -> Result<(), Unfilled>;
}

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
