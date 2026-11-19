//! Why a program stops once it has booted: it was asked to, or a root failed (ADR-0020).

use aethereal_actor::{ActorName, Fault};

/// Why the program stops once it has booted, as its lifecycle holds it while it stops and once it has (ADR-0020). A
/// host's exit says which: a quit is a clean end, and a failure a failing one (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halted {
    /// It was asked to: the host's `quit`, or an actor's `turn.quit()`.
    Quit,
    /// A turn of the root failed with the fault: it returned it, or its guest trapped. No parent hears of a root's
    /// failure, so the program stops.
    Failed(ActorName, Fault),
}
