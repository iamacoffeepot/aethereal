//! The reconcile's rules, in the order they are tried: the first that applies gives the effect.
//!
//! 1. A line of the named container's log that the transcript lacks is copied first, so every rule after reads a
//!    transcript that holds all the container wrote.
//! 2. With no container named, the session starts its next run, wakes for a line, or rests ([`off`]).
//! 3. An exited container is removed once its exit is in the record, then the next phase is written.
//! 4. A container that exited, was refused, or is gone has its end read and written before anything else is decided
//!    of it ([`ended`]).
//! 5. An archive or an owner's stop is begun; a line said after the session's hold wakes it; a hibernate stops a
//!    bootstrap, or a Claude Code container that is not yet running.
//! 6. A container being stopped is stopped, or removed when it never started.
//! 7. A container that should run is made, started, and, for a bootstrap, held to its deadline; a running Claude Code
//!    container is held to its turn's deadline, written its lines, and wound down when idle or asked to hibernate
//!    ([`claude`]).

mod claude;
mod ended;
mod off;

use super::effect::{Ask, Begin, Effect, Until};
use super::facts::{Facts, Seen, Unread};
use super::record::{Begun, Desired, Last, Overdue, Phase, Record, Run, When, Why};
use super::transcript::{Entry, Output, Role, What};
use core::ops::ControlFlow::{self, Break, Continue};

/// The one next effect for the session `facts` describes.
#[must_use]
pub fn reconcile(facts: &Facts<'_>) -> Effect {
    match decide(facts) {
        Break(effect) => effect,
        Continue(()) => Effect::Wait(Until::Event),
    }
}

fn decide(f: &Facts<'_>) -> ControlFlow<Effect> {
    let Some((role, state)) = f.record.phase.container() else {
        return off::off(f);
    };
    if let Unread::Next(output) = f.container.unread {
        return Break(copy(f.record.run, role, output));
    }
    if state == Run::Exited {
        return exited(f, role);
    }
    ended::ended(f, role, state)?;
    stop(f, role, state)?;

    match state {
        Run::Stopping(_) => stopping(f, role),
        Run::Running | Run::Exited => running(f, role),
    }
}

/// The phase `role` names, in `state`.
const fn phase(role: Role, state: Run) -> Phase {
    match role {
        Role::Bootstrap => Phase::Bootstrap(state),
        Role::Claude => Phase::Claude(state),
    }
}

/// The record with its container's state changed to `state`.
fn written(f: &Facts<'_>, role: Role, state: Run) -> Effect {
    Effect::Record(Record { phase: phase(role, state), ..*f.record })
}

/// Copies a line of `run`'s container `role`'s log into the transcript.
fn copy(run: u32, role: Role, output: &Output) -> Effect {
    Effect::Append(Entry { at: output.at, what: What::Output { run, role, event: output.event.clone() } })
}

/// Wakes a hibernating session an owner's line was said to after its hold.
fn wake(f: &Facts<'_>) -> ControlFlow<Effect> {
    let record = f.record;
    let hibernating = matches!(record.desired, Desired::Hibernating(_));
    let woken = f.ledger.wakes(record.held_through);
    if hibernating && woken {
        return Break(Effect::Record(Record { desired: Desired::Awake, ..*record }));
    }

    Continue(())
}

/// A container whose exit is in the record is removed; once it is gone, the run goes on to Claude Code after a
/// bootstrap that exited 0 while the session should be awake, and otherwise to no container.
fn exited(f: &Facts<'_>, role: Role) -> ControlFlow<Effect> {
    let record = f.record;
    let run = record.run;
    match f.container.seen {
        Seen::Created | Seen::Running { .. } | Seen::Exited { .. } => Break(Effect::Docker(Ask::Remove { run, role })),
        Seen::Absent | Seen::Refused(_) => {
            wake(f)?;
            let ended = matches!(record.last, Last::Ended { run: last, .. } if last == run);
            let awake = record.desired == Desired::Awake;
            let ready = role == Role::Bootstrap && !ended && awake;
            let phase = if ready {
                Phase::Claude(Run::Running)
            } else {
                Phase::Off
            };
            Break(Effect::Record(Record { phase, ..*record }))
        }
    }
}

/// Begins a stop the session's desire asks for at once: an archive, an owner's stop, or a hibernate of a container
/// that has nothing to wind down: a bootstrap, or a Claude Code container not yet running. A line said after the hold
/// wakes a hibernating session first, unless the owner stopped it.
fn stop(f: &Facts<'_>, role: Role, state: Run) -> ControlFlow<Effect> {
    if state != Run::Running {
        return wake(f);
    }
    let why = match f.record.desired {
        Desired::Archived => Why::Archive,
        Desired::Hibernating(When::Now) => Why::Owner,
        Desired::Awake => return Continue(()),
        Desired::Hibernating(When::AfterTurn) => {
            wake(f)?;
            let running = matches!(f.container.seen, Seen::Running { .. });
            if role == Role::Claude && running {
                return Continue(());
            }
            Why::Hibernate
        }
    };

    Break(written(f, role, Run::Stopping(why)))
}

/// A container being stopped: stopped while it runs, and removed when it was made and never started, as a stop does
/// nothing to it.
fn stopping(f: &Facts<'_>, role: Role) -> ControlFlow<Effect> {
    let run = f.record.run;
    match f.container.seen {
        Seen::Running { .. } => Break(Effect::Docker(Ask::Stop { run, role })),
        Seen::Created => Break(Effect::Docker(Ask::Remove { run, role })),
        Seen::Absent | Seen::Exited { .. } | Seen::Refused(_) => Continue(()),
    }
}

/// A container that should run: made, started, and then held to its deadlines and given its lines.
fn running(f: &Facts<'_>, role: Role) -> ControlFlow<Effect> {
    let record = f.record;
    let run = record.run;
    match f.container.seen {
        Seen::Absent => {
            let conversation = match record.begun {
                Begun::Yes => Begin::Resume,
                Begun::No => Begin::Fresh,
            };
            Break(Effect::Docker(Ask::Create { run, role, conversation }))
        }
        Seen::Created => Break(Effect::Docker(Ask::Start { run, role })),
        Seen::Running { started } => match role {
            Role::Bootstrap => {
                let due = started + f.limits.bootstrap;
                if f.now >= due {
                    return Break(written(f, role, Run::Stopping(Why::Overdue(Overdue::Bootstrap))));
                }
                Break(Effect::Wait(Until::Time(due)))
            }
            Role::Claude => claude::claude(f, started),
        },
        Seen::Exited { .. } | Seen::Refused(_) => Continue(()),
    }
}
