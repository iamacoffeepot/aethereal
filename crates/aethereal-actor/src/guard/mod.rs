//! Who may send a handler its kind: the handler's guard, which dispatch asks before the handler runs (ADR-0058).
//!
//! A mail handler states its guard as `type From`. The
//! guard is handed the receiver, the mail read as its kind, and the sender,
//! and answers the sender checked, as its [`Sent`] type, or nothing, when the
//! mail goes to the handler's `refused`. The library's guards are here; a
//! handler writes its own as it writes any impl. A native send whose target
//! names its receiver's type is checked against the guard's `Sent` type at its
//! call too ([`Admits`], ADR-0059).
//!
//! [`Parent`] and [`Child`] are reached through this module, as the crate's root names the relative paths so.

mod call;
mod library;
mod sender;
#[cfg(test)]
mod tests;

pub use call::{Admits, SentBy};
pub use library::{Anyone, Child, Itself, Parent, Speaks};
pub use sender::Sender;

use aethereal_data::KindId;

/// A guard of the actor type `A`: what its handler is handed for a sender it admits (ADR-0058).
///
/// What a guard hands on depends on the actor, never on the mail's kind, so it is named here, where a turn can name
/// it, and the check that reads the mail is [`Checks`]. `A` is bound by each guard that reads it, so a turn of any
/// actor names the default guard.
pub trait Guard<A>: 'static {
    /// What the handler is handed for a sender it admits.
    type Sent: Sent;
}

/// The guard's check of mail of `K` sent to an actor of type `A`, which may read the receiver and the mail (ADR-0058).
pub trait Checks<A, K>: Guard<A> {
    /// The sender, checked, when `actor` takes `mail` from it; `None` when it does not, and the mail goes to
    /// `refused`.
    fn admits(actor: &A, mail: &K, sender: &Sender<'_>) -> Option<Self::Sent>;
}

/// A checked sender: the types of actor it can be, and how it is taken from the sender, so one type states both
/// (ADR-0058).
///
/// `from_sender` can answer only through the [`Sender`]'s own checks, which prove what they answer, so what
/// [`TYPES`](Self::TYPES) states and what delivery admits are the same types.
pub trait Sent: Sized + 'static {
    /// The types of actor it can be.
    const TYPES: SenderTypes;

    /// The sender as this type, when it is one; `None` when it is not.
    fn from_sender(sender: &Sender<'_>) -> Option<Self>;
}

/// Which types of sender a guard may admit, as its [`Sent`] type says (ADR-0058).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderTypes {
    /// Any sender.
    Every,
    /// An actor of one of these types.
    Types(&'static [KindId]),
    /// An actor whose type lists every one of these kinds.
    Speaking(&'static [KindId]),
    /// What any of these admits: an enum's cases.
    AnyOf(&'static [Self]),
}
