//! An item: one thing an actor is handed to react to (ADR-0009).

use crate::{Ended, Mail};

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
    /// The answer to a request this actor asked, and the context it kept for it, encoded (ADR-0017). Its source is
    /// the actor asked.
    Answer { mail: Mail<'a>, context: &'a [u8] },
}
