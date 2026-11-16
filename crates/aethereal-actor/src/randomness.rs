//! The host's randomness: bytes no other process can guess, which a host serves its jobs (ADR-0034), as it hands them
//! its timer. `Engines` mints its link's tokens from it (ADR-0040), and `Sessions` each session's route bearer
//! (ADR-0057).

/// Why the host's randomness filled nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unfilled;

/// The host's randomness. A host serves it into the services its runners hand every job (ADR-0034) as it bootstraps a
/// root that draws from it; a host that cannot serve it does not bootstrap that root.
///
/// A fill never blocks the thread that polls it: a host's randomness is ready once its machine has started.
pub trait Randomness: Send + Sync {
    /// Fills `bytes` with the host's randomness.
    ///
    /// # Errors
    ///
    /// The host's randomness could not be read.
    fn fill(&self, bytes: &mut [u8]) -> Result<(), Unfilled>;
}
