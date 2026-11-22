//! The asks a session's callers make, each answered by at most one write: a line said is appended, and the rest write
//! the record's `desired`. The reconcile does everything else.

use super::effect::Write;
use super::facts::Attachment;
use super::ledger::Ledger;
use super::record::{Desired, Phase, Record, When};
use super::transcript::{Entry, LineId, What};
use aethereal_actor::Time;

/// An ask of a session's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ask {
    /// Say the line a caller typed, by its id (D1).
    Say(u128),
    /// Hibernate once no turn or task runs, after the wind-down (D8).
    Hibernate,
    /// Stop at once, whatever runs, keeping every line (D5).
    Stop,
    /// Wake.
    Wake,
    /// Archive.
    Archive,
}

/// Whether one more session may be awake under the hub's most awake at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Room {
    Free,
    Full,
}

/// How an ask is answered.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Answer {
    /// Append this entry, then answer.
    Append(Entry),
    /// Write this record, then answer.
    Record(Record),
    /// Answer at once: it already holds.
    Unchanged,
    Refused(Refused),
}

/// Why an ask is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refused {
    /// The session is archived.
    Archived,
    /// It would wake past the most awake at once.
    Full,
    /// No turn runs to interrupt, or the hub holds no attach to write it through.
    Idle,
}

/// Answers `ask` of the session `record` and `ledger` describe, at `now`, with `room` for one more awake session.
#[must_use]
pub fn answer(ask: Ask, record: &Record, ledger: &Ledger, room: Room, now: Time) -> Answer {
    let archived = matches!(record.desired, Desired::Archived);
    let may_wake = counts(record, ledger) || matches!(room, Room::Free);
    let entries = ledger.entries();

    match ask {
        Ask::Archive if archived => Answer::Unchanged,
        Ask::Archive => Answer::Record(Record { desired: Desired::Archived, ..*record }),
        _ if archived => Answer::Refused(Refused::Archived),
        Ask::Say(id) if ledger.has_said(LineId::Owner(id)) => Answer::Unchanged,
        Ask::Say(_) if !may_wake => Answer::Refused(Refused::Full),
        Ask::Say(id) => Answer::Append(Entry { at: now, what: What::Said(LineId::Owner(id)) }),
        Ask::Hibernate => match record.desired {
            Desired::Awake => {
                let desired = Desired::Hibernating(When::AfterTurn);

                Answer::Record(Record { desired, held_through: entries, ..*record })
            }
            Desired::Hibernating(_) | Desired::Archived => Answer::Unchanged,
        },
        Ask::Stop => {
            let desired = Desired::Hibernating(When::Now);
            let stopped = record.desired == desired && record.held_through == entries;
            if stopped {
                return Answer::Unchanged;
            }

            Answer::Record(Record { desired, held_through: entries, ..*record })
        }
        Ask::Wake => match record.desired {
            Desired::Awake => Answer::Unchanged,
            Desired::Hibernating(_) | Desired::Archived if !may_wake => Answer::Refused(Refused::Full),
            Desired::Hibernating(_) | Desired::Archived => {
                Answer::Record(Record { desired: Desired::Awake, ..*record })
            }
        },
    }
}

/// Whether the session counts against the most awake at once: it should be awake, a container of it may exist, or a
/// line said to it is about to wake it.
#[must_use]
pub fn counts(record: &Record, ledger: &Ledger) -> bool {
    match record.desired {
        Desired::Awake => true,
        Desired::Hibernating(_) => !matches!(record.phase, Phase::Off) || ledger.wakes(record.held_through),
        Desired::Archived => !matches!(record.phase, Phase::Off),
    }
}

/// Answers `interrupt_session`: the interrupt is written through the hub's attach when a turn runs in the run it is
/// attached to, and never again (D4).
pub fn interrupt(record: &Record, ledger: &Ledger, attachment: &Attachment) -> Result<Write, Refused> {
    let attached = matches!(attachment, Attachment::Open { run, .. } if *run == record.run);
    let turning = ledger.activity(record.run).turning();
    if attached && turning {
        return Ok(Write::Interrupt);
    }

    Err(Refused::Idle)
}
