//! How a run's container ended, read before anything else is decided of it: an exit Docker shows, an ask Docker
//! refused, or a container gone. The end is written in one record write, with the container's state `Exited`, after
//! the transcript holds all it wrote and, when Claude Code was cut short or lost, the hub's lost line.

use super::phase;
use crate::reconcile::effect::Effect;
use crate::reconcile::facts::{Facts, Seen};
use crate::reconcile::ledger::{Read, Resume};
use crate::reconcile::record::{Begun, Desired, How, Last, Overdue, Reason, Record, Refusal, Run, When, Why};
use crate::reconcile::transcript::{Entry, LineId, Role, What};
use aethereal_actor::Time;
use core::ops::ControlFlow::{self, Break, Continue};

/// How a container came to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Exited {
        code: i32,
        oom: bool,
    },
    Refused(Refusal),
    /// It was gone, made or not.
    Gone,
}

/// Reads the end of the container `role` in `state`, when it has one.
pub(super) fn ended(f: &Facts<'_>, role: Role, state: Run) -> ControlFlow<Effect> {
    let run = f.record.run;
    match f.container.seen {
        Seen::Exited { code, oom, finished } => Break(end(f, role, state, Ending::Exited { code, oom }, finished)),
        Seen::Refused(refusal) => Break(end(f, role, state, Ending::Refused(refusal), f.now)),
        Seen::Absent => {
            // A container that wrote output, or that the hub attached to, ran: absent, it is gone with whatever it had
            // read. One that did neither has read nothing, so it is made again.
            let wrote = f.ledger.container(run, role).is_some();
            let attached = role == Role::Claude && f.ledger.marker(run).is_some();
            let ran = wrote || attached;
            let stopping = matches!(state, Run::Stopping(_));
            if !ran && !stopping {
                return Continue(());
            }
            let noted = f.ledger.gone(run, role);
            if ran && !noted {
                return Break(Effect::Append(Entry { at: f.now, what: What::Gone { run, role } }));
            }
            Break(end(f, role, state, Ending::Gone, f.now))
        }
        Seen::Created | Seen::Running { .. } => Continue(()),
    }
}

/// The write that ends the container `role` of the record's run, in `state`, as `ending` says, at `finished`: or the
/// lost line first, when Claude Code was lost or cut a line it read short.
fn end(f: &Facts<'_>, role: Role, state: Run, ending: Ending, finished: Time) -> Effect {
    let record = f.record;
    let run = record.run;
    let copied = f.ledger.container(run, role);
    let how = how(f, role, state, ending);
    // Claude is told when its container was lost, when it cut short a line it read, or when its output is gone, so
    // what it read is not known.
    let claude = role == Role::Claude;
    let cut = f.ledger.activity(run).unanswered() || f.ledger.gone(run, role);
    let warn = how == Outcome::Ended(How::Lost) || (claude && cut);
    let warned = f.ledger.has_said(LineId::Lost { run });
    if warn && !warned {
        return Effect::Append(Entry { at: f.now, what: What::Said(LineId::Lost { run }) });
    }
    let read = copied.is_some_and(|copied| copied.read == Read::Something);
    let begun = match how {
        _ if read => Begun::Yes,
        Outcome::Ended(How::Unstarted(Reason::Unresumable)) => Begun::No,
        Outcome::Ended(How::Unstarted(Reason::InUse)) => Begun::Yes,
        Outcome::Ready | Outcome::Ended(_) => record.begun,
    };
    let mut next = Record { phase: phase(role, Run::Exited), begun, ..*record };
    let Outcome::Ended(how) = how else {
        return Effect::Record(next);
    };
    next.last = Last::Ended { run, how };
    // A failure puts an awake session to sleep, and holds every line said before the container ended: nothing
    // starts it again but a wake or a line said after.
    let fails = how.fails();
    let archived = record.desired == Desired::Archived;
    if fails && !archived {
        if record.desired == Desired::Awake {
            next.desired = Desired::Hibernating(When::AfterTurn);
        }
        next.held_through = record.held_through.max(f.ledger.said_through(finished));
    }

    Effect::Record(next)
}

/// What a container's end means for its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// A bootstrap exited 0: the run goes on to Claude Code.
    Ready,
    /// The run ended so.
    Ended(How),
}

/// What the end of the container `role`, in `state`, as `ending` says, means for the record's run.
fn how(f: &Facts<'_>, role: Role, state: Run, ending: Ending) -> Outcome {
    let record = f.record;
    let copied = f.ledger.container(record.run, role);
    let unresumed = copied.is_some_and(|copied| copied.resume == Resume::Failed);
    let silent = copied.is_none() && record.begun == Begun::No;
    let how = match (role, state, ending) {
        (Role::Bootstrap, Run::Stopping(Why::Overdue(Overdue::Bootstrap)), _) => {
            How::Unstarted(Reason::BootstrapOverdue)
        }
        (_, Run::Stopping(why), _) => How::Stopped(why),
        (_, _, Ending::Refused(refusal)) => How::Unstarted(Reason::Refused(refusal)),
        (Role::Bootstrap, _, Ending::Gone) => How::Unstarted(Reason::Gone),
        (Role::Bootstrap, _, Ending::Exited { code: 0, oom: false }) => return Outcome::Ready,
        (Role::Bootstrap, _, Ending::Exited { oom: true, .. }) => How::Unstarted(Reason::BootstrapOutOfMemory),
        (Role::Bootstrap, _, Ending::Exited { code, .. }) => How::Unstarted(Reason::Bootstrap { code }),
        (Role::Claude, _, Ending::Exited { .. }) if unresumed => How::Unstarted(Reason::Unresumable),
        (Role::Claude, _, Ending::Exited { code: 1, oom: false }) if silent => How::Unstarted(Reason::InUse),
        (Role::Claude, _, Ending::Exited { .. } | Ending::Gone) => How::Lost,
    };

    Outcome::Ended(how)
}
