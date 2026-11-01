//! A tool as a program lists it (ADR-0037).

use crate::{Offer, ToolSheet, Toolbox, Tooling};
use aethereal_actor::{Actor, ActorRef, Anywhere, Beginning, ChildOf, Fault, ProtocolRef};

/// A worker a program may list in a [`Toolbox`]: a concrete actor, written in the crate that owns the work it fronts.
pub trait Tool: Actor<Key = (), Config: Default> + ChildOf<Parent = Anywhere> {
    /// What it offers.
    fn sheets() -> Vec<ToolSheet>;

    /// This tool as a worker: `me.protocol()`, which builds only when it speaks [`Tooling`].
    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling>;
}

/// The tools of a [`Toolbox`]: a tuple of [`Tool`]s.
pub trait ToolList: Send + 'static {
    /// Every tool's sheets, in the order listed.
    fn sheets() -> Vec<ToolSheet>;

    /// Spawns each tool, beneath the toolbox `turn` begins, and answers with what each offers.
    ///
    /// # Errors
    ///
    /// A fault naming the tool whose spawn was refused.
    fn stand_up<L: ToolList, S>(turn: &mut Beginning<'_, Toolbox<L>, S>) -> Result<Vec<Offer>, Fault>;
}
