//! When a request stops being waited for: the deadline it carries, which the requests made on its behalf inherit
//! (ADR-0041).

use crate::Time;
use aethereal_data::Plain;

/// When a request stops being waited for (ADR-0041).
///
/// At its deadline the engine answers the asker [`Unanswered::Expired`](crate::Unanswered::Expired) and forgets the
/// request, and an answer that comes later is dropped. An ask made in a turn that handles a request inherits that
/// request's `Due`, unless it states its own with [`within`](crate::Toward::within); one from a `begin`, or from a turn
/// that handles mail that is not a request, is due `Never` unless it states one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Due {
    /// Never: it waits until it is answered, as every request did before deadlines.
    Never,
    /// At this time on the engine's clock: the host's, which a turn reads as [`now`](crate::Turn::now).
    At(Time),
}

/// A deadline at a time.
impl From<Time> for Due {
    fn from(at: Time) -> Self {
        Self::At(at)
    }
}
