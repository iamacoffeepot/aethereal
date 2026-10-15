//! A piece of mail as the engine carries it (ADR-0012).

use aethereal_actor::{MailId, Payload, SeatNumber};
use aethereal_data::KindId;

/// A piece of mail as the engine carries it: what it is, the seat it comes from, and what it follows from
/// (ADR-0012).
///
/// It holds nothing the engine keeps beside it: the stream's entry names the
/// actor it is for and the mail's id, its sender and that sender's count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    /// The seat of the actor that sent it.
    pub sender_seat: SeatNumber,
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
