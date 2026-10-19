//! An item: one thing an actor is handed to react to (ADR-0009).

use crate::{Asking, Ended, Mail};

/// One thing an actor is handed to react to, from the stream of them the engine keeps for it (ADR-0009).
///
/// An actor is run in a step when its stream has items, and each item is a
/// turn of its own. Each item has a source, the actor it comes from, which is
/// who a send to [`To::Sender`](crate::To::Sender) reaches.
#[derive(Clone)]
pub enum Item<'a> {
    /// A piece of mail. Its source is its sender.
    Mail(Mail<'a>),
    /// An actor this one watches has ended. Its source is that actor, so a send to it is not delivered.
    Ended(Ended),
    /// The answer to a request this actor asked, and what the engine kept of the request: the context it kept for it
    /// (ADR-0017). Its source is the actor asked.
    Answer { mail: Mail<'a>, asking: &'a Asking },
    /// What the engine says of a request this actor asked that was not answered, an
    /// [`Unanswered`](crate::Unanswered), and what the engine kept of the request: the kind it was to be answered
    /// with, whose handler it is handed to, and the context it kept for it (ADR-0017). Its source is the actor asked.
    Unanswered { mail: Mail<'a>, asking: &'a Asking },
}
