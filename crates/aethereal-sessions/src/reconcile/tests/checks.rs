//! The invariants checked in every state the explorer reaches, and in every quiet one.

use super::actions::{Action, effect};
use super::bounds::Bounds;
use super::world::{Docked, Doing, Process, Tasks, World};
use crate::reconcile::{Desired, Effect, LineId, Phase, Role, Run, Until, What};
use aethereal_actor::Time;

/// Checks the invariants every state keeps.
pub(super) fn state(world: &World, bounds: &Bounds) -> Result<(), String> {
    let housed = world.sessions.iter().filter(|session| !session.containers.is_empty()).count();
    if housed > bounds.cap {
        return Err(format!("{housed} sessions have containers, past the most awake, {}", bounds.cap));
    }
    for (index, session) in world.sessions.iter().enumerate() {
        // I2: at most one container, the one the record names.
        let keys: Vec<&(u32, Role)> = session.containers.keys().collect();
        let named = session.named();
        let stray = keys.iter().any(|key| Some(**key) != named);
        if keys.len() > 1 || stray {
            return Err(format!("session {index} has containers {keys:?} while its record names {named:?}"));
        }
        // D1: a line said twice is one entry.
        let mut owners: Vec<LineId> = session
            .transcript
            .iter()
            .filter_map(|entry| match entry.what {
                What::Said(line @ LineId::Owner(_)) => Some(line),
                _ => None,
            })
            .collect();
        let said = owners.len();
        owners.dedup();
        if owners.len() != said {
            return Err(format!("session {index} holds an owner's line twice"));
        }
        deadlines(world, index, bounds)?;
    }

    Ok(())
}

/// I8: a wait ends no later than every deadline that is running, as measured from what happened, never from the hub's
/// memory, so no restart puts one off.
fn deadlines(world: &World, index: usize, bounds: &Bounds) -> Result<(), String> {
    let Effect::Wait(until) = effect(world, index, bounds) else {
        return Ok(());
    };
    let session = &world.sessions[index];
    let limits = bounds.limits;
    let mut due: Vec<(Time, &str)> = Vec::new();
    let running = matches!(session.record.phase.container(), Some((_, Run::Running)));
    let container = session.named().and_then(|key| session.containers.get(&key).map(|container| (key, container)));
    if let Some(((run, role), container)) = container
        && let Docked::Running { started } = container.state
        && running
    {
        match (role, &container.process) {
            (Role::Bootstrap, _) => due.push((started + limits.bootstrap, "the bootstrap's deadline")),
            (Role::Claude, Process::Live(claude)) => {
                let since = match (claude.doing, claude.tasks) {
                    (Doing::Turn { since, .. } | Doing::Compacting { since, .. }, _) | (_, Tasks::One { since }) => {
                        Some(since)
                    }
                    _ => None,
                };
                if let Some(since) = since {
                    let ledger = session.ledger();
                    let interrupted =
                        ledger.activity(run).anchor().and_then(|anchor| ledger.interrupt(run, anchor.number).copied());
                    let deadline =
                        interrupted.map_or(since + limits.turn, |interrupt| interrupt.mark.at + limits.grace);
                    due.push((deadline, "the turn's deadline"));
                }
                let idle =
                    matches!(claude.doing, Doing::Idle) && claude.queue.is_empty() && claude.tasks == Tasks::None;
                let awake = session.record.desired == Desired::Awake;
                if idle && awake && container.pipe.is_empty() {
                    let latest = container.log.iter().map(|output| output.at).max().unwrap_or(started).max(started);
                    due.push((latest + limits.idle, "the idle time"));
                }
            }
            (Role::Claude, _) => {}
        }
    }
    match until {
        Until::Event => match due.first() {
            Some((at, what)) => Err(format!("session {index} waits with no deadline while {what} falls at {at:?}")),
            None => Ok(()),
        },
        Until::Time(at) if at <= world.now => Err(format!("session {index} waits until {at:?}, which has passed")),
        Until::Time(at) => match due.iter().find(|(deadline, _)| *deadline < at) {
            Some((deadline, what)) => Err(format!("session {index} waits until {at:?}, past {what} at {deadline:?}")),
            None => Ok(()),
        },
    }
}

/// Whether nothing more happens in `world` without an ask, a fault, or time: every session's reconcile waits, and no
/// container's process has a step to take.
pub(super) fn quiet(actions: &[Action]) -> bool {
    actions.iter().all(|action| !action.calm() || *action == Action::Tick)
}

/// I3 and the lines' fate, in a quiet world: the record and Docker agree, and every line said was read or is held.
pub(super) fn settled(world: &World) -> Result<(), String> {
    for (index, session) in world.sessions.iter().enumerate() {
        let record = session.record;
        let awake = record.desired == Desired::Awake;
        let running = session.named() == Some((record.run, Role::Claude))
            && matches!(record.phase.container(), Some((_, Run::Running)))
            && session.containers.get(&(record.run, Role::Claude)).is_some_and(super::world::Container::running);
        let off = matches!(record.phase, Phase::Off) && session.containers.is_empty();
        if awake && !running {
            return Err(format!("session {index} is quiet and should be awake, but its Claude Code does not run"));
        }
        if !awake && !off {
            return Err(format!(
                "session {index} is quiet and should be {:?}, but a container of it may exist",
                record.desired
            ));
        }
        let archived = record.desired == Desired::Archived;
        let ledger = session.ledger();
        for &run in &session.truth.cut {
            let lost = LineId::Lost { run };
            let warned = if awake {
                session.truth.saved.contains(&lost)
            } else {
                ledger.has_said(lost)
            };
            if !warned && !archived {
                return Err(format!(
                    "session {index} is quiet, and run {run} cut a line short with no lost line to tell it"
                ));
            }
        }
        for said in ledger.said() {
            let owner = matches!(said.line, LineId::Owner(_));
            let read = session.truth.saved.contains(&said.line);
            let held = !awake && said.number <= record.held_through;
            if owner && !read && !held && !archived {
                return Err(format!("session {index} is quiet, and {:?} is neither read nor held", said.line));
            }
        }
    }

    Ok(())
}
