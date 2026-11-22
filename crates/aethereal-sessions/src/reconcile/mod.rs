//! The session lifecycle's reconcile (ADR-0057): one pure function that reads what the hub knows of one session, its
//! record, its transcript, Docker's state of the one container the record names, and what the hub holds in memory of
//! that container's stdin, and returns the one next effect that brings them into line.
//!
//! It is the whole of what the hub decides about a session's containers. Each effect is one write, the record whole,
//! one transcript entry, one Docker ask, or one write to Claude's stdin, and each can be repeated, so a hub that stops
//! between any two picks up where its files say it was by calling it again, and runs nothing else. The asks of a
//! session's callers are answered by [`answer`] and [`interrupt`], each with at most one write of their own.
//!
//! The hub does not call it yet: it reads the record and transcript in the shapes the sessions capability moves to
//! next. Its tests explore every interleaving of the owner's asks, the hub's effects and crashes, Docker's faults,
//! Claude Code's steps as probed, and the clock, within bounds, and check its invariants in every state reached.

mod asks;
mod effect;
mod facts;
mod ledger;
mod record;
mod rules;
mod transcript;

#[cfg(test)]
mod tests;

pub use asks::{Answer, Ask as Asked, Refused, Room, answer, counts, interrupt};
pub use effect::{Ask, Begin, Effect, Until, Write};
pub use facts::{Attachment, Container, Facts, Limits, Seen, Unread};
pub use ledger::{Activity, Compaction, Copied, Interrupted, Ledger, Mark, Read, Resume, Said, Since, Turn};
pub use record::{Begun, Desired, How, Last, Overdue, Phase, Reason, Record, Refusal, Run, When, Why};
pub use rules::reconcile;
pub use transcript::{Entry, Event, LineId, Output, Role, TaskId, What};
