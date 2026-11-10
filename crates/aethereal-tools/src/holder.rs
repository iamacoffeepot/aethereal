//! A root that holds tools (ADR-0038, ADR-0053).

use crate::{Offer, Offered, Withdraw};
use aethereal_actor::{Actor, Carries, ChildOf, Handles, PathTo, RepliesWith, Top};

/// A root that holds the tools offered to it, and asks each call of the worker that offered it: `Mcp` in a hub, and
/// [`Tools`](crate::Tools) in an engine (ADR-0038, ADR-0053). It answers an [`Offer`] with [`Offered`], in the turn that asks, and takes a
/// [`Withdraw`], answering nothing to whoever sent it.
///
/// A [`Toolbox`](crate::Toolbox) names the holder it offers to, and is checked against it at build as it is against
/// any root it reaches.
pub trait Holder:
    Actor<Key = ()>
    + ChildOf<Parent = Top>
    + PathTo<Keys = ()>
    + Carries<Offer, Answers = (Offered,)>
    + Handles<Offer, Sends: RepliesWith<Offered>>
    + Handles<Withdraw>
{
}
