//! A piece of mail as the engine carries it (ADR-0012).

use aethereal_actor::MailId;
use aethereal_data::{ActorId, KindId};

/// A piece of mail as the engine carries it: who it is for, what it is, and what it follows from (ADR-0012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The mail's own id.
    pub id: MailId,
    /// The actor it is for.
    pub to: ActorId,
    /// The mail whose turn sent this one. None when nothing inside the engine caused it, and nothing ever will
    /// have: fixed when the mail is made.
    pub cause: Option<MailId>,
    /// The first mail of the chain this one belongs to.
    pub root: MailId,
    /// What is owed for it, or what it answers.
    pub tie: Tie,
    /// The id of its kind.
    pub kind: KindId,
    /// Its contents, as the bytes of that kind.
    pub bytes: Vec<u8>,
}

/// How a piece of mail is tied to a request (ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tie {
    /// Nothing is owed for it.
    Tell,
    /// An answer is owed.
    Ask,
    /// One part of the stream that request opened.
    Part { request: MailId },
    /// The answer to that request.
    Answer { request: MailId },
}
