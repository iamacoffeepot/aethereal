//! Why a program's boot failed: an actor quit, or a turn failed, before `Start` (ADR-0020, ADR-0029).

use crate::stages::Halt;
use aethereal_actor::{ActorName, Fault};
use core::error::Error;
use core::fmt;

/// Why the program's boot failed: what an actor did while `Boot`'s chain ran, before `Start` was sent (ADR-0020). A
/// host reports it as a program that did not start, naming the actor (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbooted {
    /// The actor asked the program to stop.
    Quit(ActorName),
    /// A turn of the actor failed with the fault: it returned it, or its guest trapped.
    Failed(ActorName, Fault),
}

impl Unbooted {
    /// What `halt`, by the actor named `actor`, fails the boot with.
    pub(super) fn of(halt: Halt, actor: ActorName) -> Self {
        match halt {
            Halt::Quit(_) => Self::Quit(actor),
            Halt::Failed(_, fault) => Self::Failed(actor, fault),
        }
    }
}

impl fmt::Display for Unbooted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quit(actor) => write!(f, "{} quit before the program started", Shown(actor)),
            Self::Failed(actor, fault) => write!(
                f,
                "{} failed before the program started, with a fault of kind {:?}, {} bytes",
                Shown(actor),
                fault.kind(),
                fault.bytes().len()
            ),
        }
    }
}

impl Error for Unbooted {}

/// An actor's name as a report shows it: its path, or its id's bits when the table no longer remembers the path, as
/// the tools' reads of the log show it.
struct Shown<'a>(&'a ActorName);

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ActorName::Path(path) => write!(f, "{path}"),
            ActorName::Unnamed(id) => write!(f, "{:#018x}", id.bits()),
        }
    }
}
