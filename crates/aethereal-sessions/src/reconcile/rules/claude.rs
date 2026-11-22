//! A running Claude Code container: held to its turn's deadline, written the lines it has not read, and wound down
//! when it is idle or the session is asked to hibernate.
//!
//! The hub writes to its stdin only through an attach whose marker it appended itself. A marker it does not hold means
//! another attach, of a hub that stopped or of a stream that ended, may have left a line cut short in the pipe, and
//! anything written after it would end Claude Code: so that container is never written again. It is stopped once
//! nothing runs in it, and the next run writes its lines.

use super::written;
use crate::reconcile::effect::{Ask, Effect, Until, Write};
use crate::reconcile::facts::{Attachment, Facts};
use crate::reconcile::ledger::{Activity, Said};
use crate::reconcile::record::{Desired, Overdue, Record, Run, When, Why};
use crate::reconcile::transcript::{Entry, LineId, Role, What};
use aethereal_actor::Time;
use core::ops::ControlFlow::{self, Break, Continue};

/// The hub's attach to the run's stdin, as its memory and the transcript's markers together say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attach<'a> {
    /// Open under the latest marker, which the hub appended: it has written `wrote` through it, after the marker
    /// numbered `marker`.
    Own { marker: u64, wrote: &'a [Write] },
    /// The hub appended the latest marker, and attaches next.
    Marked,
    /// No marker: nothing was ever written to this stdin.
    Fresh,
    /// A marker the hub does not hold: the stdin may hold a line cut short.
    Stale,
}

impl<'a> Attach<'a> {
    fn of(f: &'a Facts<'_>) -> Self {
        let run = f.record.run;
        let Some(latest) = f.ledger.marker(run) else {
            return Self::Fresh;
        };
        match f.attachment {
            Attachment::Open { run: open, marker, wrote } if *open == run && *marker == latest => {
                Self::Own { marker: *marker, wrote }
            }
            Attachment::Marked { run: marked, marker } if *marked == run && *marker == latest => Self::Marked,
            Attachment::Detached | Attachment::Marked { .. } | Attachment::Open { .. } => Self::Stale,
        }
    }

    /// Whether `write` went through it.
    fn wrote(self, write: Write) -> bool {
        match self {
            Self::Own { wrote, .. } => wrote.contains(&write),
            Self::Marked | Self::Fresh | Self::Stale => false,
        }
    }

    /// Whether a line written through it has not been read.
    fn unread(self, f: &Facts<'_>) -> bool {
        match self {
            Self::Own { wrote, .. } => wrote.iter().any(|write| match write {
                Write::Line(line) => !f.ledger.receipted(*line),
                Write::Interrupt => false,
            }),
            Self::Marked | Self::Fresh | Self::Stale => false,
        }
    }
}

/// The session's running Claude Code container, started at `started`.
pub(super) fn claude(f: &Facts<'_>, started: Time) -> ControlFlow<Effect> {
    let activity = f.ledger.activity(f.record.run);
    let attach = Attach::of(f);
    deadline(f, activity, attach)?;
    let busy = activity.busy() || attach.unread(f);

    match f.record.desired {
        Desired::Awake => awake(f, activity, attach, busy, started),
        Desired::Hibernating(When::AfterTurn) => wind_down(f, activity, attach, busy),
        Desired::Hibernating(When::Now) | Desired::Archived => Continue(()),
    }
}

/// When the current turn, or task, runs out: its start and the turn's limit, or, once interrupted, the interrupt and
/// its grace.
fn turn_due(f: &Facts<'_>, activity: &Activity) -> Option<Time> {
    let anchor = activity.anchor()?;
    let interrupted = f.ledger.interrupt(f.record.run, anchor.number);
    let due = interrupted.map_or(anchor.at + f.limits.turn, |interrupted| interrupted.mark.at + f.limits.grace);

    Some(due)
}

/// A turn, or a task, past its deadline is interrupted, once, its interrupt appended before it is written; past its
/// grace, its container is stopped.
fn deadline(f: &Facts<'_>, activity: &Activity, attach: Attach<'_>) -> ControlFlow<Effect> {
    let run = f.record.run;
    let Some(anchor) = activity.anchor() else {
        return Continue(());
    };
    if f.now < anchor.at + f.limits.turn {
        return Continue(());
    }
    let Some(interrupted) = f.ledger.interrupt(run, anchor.number) else {
        let what = What::Interrupted { run, turn: anchor.number };
        return Break(Effect::Append(Entry { at: f.now, what }));
    };
    if f.now >= interrupted.mark.at + f.limits.grace {
        return Break(written(f, Role::Claude, Run::Stopping(Why::Overdue(Overdue::Turn))));
    }
    // Only an attach that appended the interrupt writes it, once: a hub that starts again never writes it again (D4).
    let ours = matches!(attach, Attach::Own { marker, .. } if interrupted.mark.number > marker);
    let written = attach.wrote(Write::Interrupt);
    if ours && !written {
        return Break(Effect::Stdin(Write::Interrupt));
    }

    Continue(())
}

/// Waits for the soonest of `times`, or for something new when there are none.
fn soonest(times: impl IntoIterator<Item = Option<Time>>) -> Effect {
    let until = times.into_iter().flatten().min().map_or(Until::Event, Until::Time);

    Effect::Wait(until)
}

/// Writes `write` through the attach, attaching first, or, when the stdin may hold a line cut short, stops the
/// container for `why` once nothing runs in it.
fn send(f: &Facts<'_>, attach: Attach<'_>, write: Write, busy: bool, why: Why) -> Effect {
    let run = f.record.run;
    match attach {
        Attach::Own { .. } => Effect::Stdin(write),
        Attach::Marked => Effect::Docker(Ask::Attach { run }),
        Attach::Fresh => Effect::Append(Entry { at: f.now, what: What::Attached { run } }),
        Attach::Stale if busy => soonest([turn_due(f, f.ledger.activity(run))]),
        Attach::Stale => written(f, Role::Claude, Run::Stopping(why)),
    }
}

/// An awake session's container: the lost lines it has not read, then the owner's, in order; once idle long enough,
/// the session hibernates.
fn awake(f: &Facts<'_>, activity: &Activity, attach: Attach<'_>, busy: bool, started: Time) -> ControlFlow<Effect> {
    let lost = f.ledger.said().iter().filter(|said| matches!(said.line, LineId::Lost { .. }));
    let owners = f.ledger.said().iter().filter(|said| matches!(said.line, LineId::Owner(_)));
    let next = lost
        .chain(owners)
        .map(|said| said.line)
        .find(|line| !f.ledger.receipted(*line) && !attach.wrote(Write::Line(*line)));
    if let Some(line) = next {
        return Break(send(f, attach, Write::Line(line), busy, Why::Reattach));
    }
    if busy {
        return Break(soonest([turn_due(f, activity)]));
    }
    let latest = f.ledger.container(f.record.run, Role::Claude).map_or(started, |copied| copied.latest.max(started));
    let idle = latest + f.limits.idle;
    if f.now >= idle {
        let desired = Desired::Hibernating(When::AfterTurn);
        return Break(Effect::Record(Record { desired, held_through: f.ledger.entries(), ..*f.record }));
    }

    Break(Effect::Wait(Until::Time(idle)))
}

/// The latest line of `kind` in this run said after the hold: this wind-down's notice or compact.
fn latest(f: &Facts<'_>, kind: impl Fn(LineId) -> bool) -> Option<Said> {
    f.ledger.said().iter().rev().find(|said| said.number > f.record.held_through && kind(said.line)).copied()
}

/// How many lines of `kind` this run has said, which numbers the next.
fn count(f: &Facts<'_>, kind: impl Fn(LineId) -> bool) -> u32 {
    let said = f.ledger.said().iter().filter(|said| kind(said.line)).count();

    u32::try_from(said).unwrap_or(u32::MAX)
}

/// A session asked to hibernate, by its owner or its idle time, once nothing runs in it: told, if it turned since its
/// last compaction, then compacted, then stopped (D8). Each step has its deadline, past which the container is stopped
/// anyway; and when the stdin may hold a line cut short, it is stopped with no wind-down.
fn wind_down(f: &Facts<'_>, activity: &Activity, attach: Attach<'_>, busy: bool) -> ControlFlow<Effect> {
    let run = f.record.run;
    let notice_of = |line| matches!(line, LineId::Notice { run: of, .. } if of == run);
    let compact_of = |line| matches!(line, LineId::Compact { run: of, .. } if of == run);
    let notice = latest(f, notice_of);
    let compact = latest(f, compact_of);
    let hibernate = written(f, Role::Claude, Run::Stopping(Why::Hibernate));
    let notice_due = notice.map(|said| said.at + f.limits.notice);
    let compact_due = compact.map(|said| said.at + f.limits.compaction);
    let compacted = compact.is_some_and(|said| f.ledger.receipted(said.line));
    let overdue = compact_due.or(notice_due).is_some_and(|due| f.now >= due);
    if (compacted && !busy) || overdue {
        return Break(hibernate);
    }
    let unwritten = [notice, compact]
        .into_iter()
        .flatten()
        .map(|said| said.line)
        .find(|line| !f.ledger.receipted(*line) && !attach.wrote(Write::Line(*line)));
    if let Some(line) = unwritten {
        return Break(send(f, attach, Write::Line(line), busy, Why::Hibernate));
    }
    let due = compact_due.or(notice_due);
    if busy {
        return Break(soonest([turn_due(f, activity), due]));
    }
    let said = match (notice, compact) {
        (Some(notice), None) if f.ledger.receipted(notice.line) => LineId::Compact { run, nth: count(f, compact_of) },
        (_, Some(_)) | (Some(_), None) => return Break(soonest([due])),
        (None, None) if f.ledger.turned() => LineId::Notice { run, nth: count(f, notice_of) },
        (None, None) => return Break(hibernate),
    };
    if attach == Attach::Stale {
        return Break(hibernate);
    }

    Break(Effect::Append(Entry { at: f.now, what: What::Said(said) }))
}
