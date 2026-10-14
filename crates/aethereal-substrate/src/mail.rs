//! A piece of mail as the engine carries it (ADR-0012).

use aethereal_actor::{MailId, Payload, SeatNumber};
use aethereal_data::{ActorId, KindId};

/// A piece of mail as the engine carries it: who it is for, what it is, and what it follows from (ADR-0012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The mail's own id.
    pub id: MailId,
    /// The seat of the actor that sent it.
    pub sender_seat: SeatNumber,
    /// The actor it is for.
    pub to: ActorId,
    /// The mail whose turn sent this one. None for mail from outside the engine, or from a root's `begin`: fixed
    /// when the mail is made.
    pub cause: Option<MailId>,
    /// The first mail of the chain this one belongs to.
    pub root: MailId,
    /// How far down its chain it is: one more than its cause's.
    pub depth: u32,
    /// The id of its kind.
    pub kind: KindId,
    /// Where its contents, the bytes of that kind, are: in the arena of the worker that sent it, or their own.
    pub bytes: Payload,
}
