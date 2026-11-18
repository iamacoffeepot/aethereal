//! A tool as a program lists it (ADR-0037).

use crate::{Holder, Offer, ToolSheet, Toolbox, Tooling};
use aethereal_actor::{Actor, ActorRef, Anywhere, Beginning, ChildOf, Fault, Pick, ProtocolRef, Spawns};

/// A worker a program may list in a [`Toolbox`]: a concrete actor, written in the crate that owns the work it fronts.
pub trait Tool: Actor<Key = (), Config: Default> + ChildOf<Parent = Anywhere> {
    /// What it offers.
    fn sheets() -> Vec<ToolSheet>;

    /// This tool as a worker: `me.protocol()`, which builds only when it speaks [`Tooling`].
    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling>;
}

/// The tools of a [`Toolbox`]: a [`Tool`], or a tuple of up to eight lists, so a list longer than eight is written as
/// groups, `((A, B, C), (D, E))`, nested as deep as it needs. It is a list of the types it lists, which a toolbox
/// spawns (ADR-0059).
pub trait ToolList: Spawns + Send + 'static {
    /// Every tool's sheets, in the order listed.
    fn sheets() -> Vec<ToolSheet>;

    /// Spawns each tool, beneath the toolbox `turn` begins, whose list `L` holds this one at the path `P`, and answers
    /// with what each offers.
    ///
    /// # Errors
    ///
    /// A fault naming the tool whose spawn was refused.
    fn stand_up<L: ToolList, R: Holder, P: Pick<L, Picked = Self>>(
        turn: &mut Beginning<'_, Toolbox<L, R>>,
    ) -> Result<Vec<Offer>, Fault>;
}
