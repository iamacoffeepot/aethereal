//! What can happen next, and what it does to the world.

use super::bounds::{ARCHIVE, Bounds, Fault, HIBERNATE, INTERRUPT, STOP, WAKE};
use super::claude::{self, Step};
use super::world::{Cause, Container, Docked, Ended, Image, Piped, Process, Refused, Repository, Session, World};
use crate::reconcile::{
    Answer, Ask, Asked, Attachment, Container as Seen2, Desired, Effect, Event, Facts, How, Last, Ledger, LineId,
    Phase, Record, Refusal, Role, Room, Run, Seen, Unread, What, Why, Write, answer, counts, interrupt, reconcile,
};
use aethereal_actor::Time;
use core::time::Duration;

/// One thing that can happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Action {
    /// The owner says their `line`th line, or says it again.
    Say {
        session: u8,
        line: u32,
    },
    /// The owner asks something of a session.
    Ask {
        session: u8,
        ask: Asked,
    },
    /// The owner interrupts a session's turn.
    Interrupt {
        session: u8,
    },
    /// The hub does what the reconcile returns for a session.
    Hub {
        session: u8,
    },
    /// The hub dies in the middle of the stdin write the reconcile returns for a session, and starts again.
    Torn {
        session: u8,
    },
    /// The hub dies and starts again: at once, or after an outage long enough for every deadline to pass.
    Crash {
        outage: bool,
    },
    /// Claude Code, in a session's running container, takes a step.
    Claude {
        session: u8,
        step: Step,
    },
    /// A session's bootstrap exits 0.
    Bootstrapped {
        session: u8,
    },
    Fault {
        session: u8,
        fault: Fault,
    },
    /// The clock moves on, while the hub has nothing to do: its own steps take no time beside a deadline.
    Tick,
}

impl Action {
    /// Whether it happens with no fault and no ask of the owner's.
    pub const fn calm(self) -> bool {
        match self {
            Self::Hub { .. } | Self::Bootstrapped { .. } | Self::Tick => true,
            Self::Claude { step, .. } => step.calm(),
            Self::Say { .. }
            | Self::Ask { .. }
            | Self::Interrupt { .. }
            | Self::Torn { .. }
            | Self::Crash { .. }
            | Self::Fault { .. } => false,
        }
    }

    /// Whether the clock moves on with it.
    pub const fn ticks(self) -> bool {
        matches!(self, Self::Tick | Self::Crash { outage: true })
    }

    /// Whether it is sure to happen while it can: the hub acting, and Claude reading its stdin.
    pub const fn fair(self) -> bool {
        match self {
            Self::Hub { .. } => true,
            Self::Claude { step, .. } => step.fair(),
            Self::Say { .. }
            | Self::Ask { .. }
            | Self::Interrupt { .. }
            | Self::Torn { .. }
            | Self::Crash { .. }
            | Self::Bootstrapped { .. }
            | Self::Fault { .. }
            | Self::Tick => false,
        }
    }
}

/// What an action came to.
pub(super) enum Outcome {
    Next(World),
    /// It would pass a bound.
    Pruned,
    /// It broke an invariant, as this says.
    Violation(String),
}

/// What an action did.
pub(super) enum Applied {
    Done,
    Pruned,
}

/// The effect the reconcile returns for session `index`.
pub(super) fn effect(world: &World, index: usize, bounds: &Bounds) -> Effect {
    let session = &world.sessions[index];
    let ledger = session.ledger();
    let empty = Unread::Nothing;
    let container = match session.named() {
        None => Seen2 { seen: Seen::Absent, unread: empty },
        Some((run, role)) => {
            let running = matches!(session.record.phase.container(), Some((_, Run::Running)));
            match (session.memory.refused, session.containers.get(&(run, role))) {
                (Refused::Refused { run: refused, role: of, refusal }, _)
                    if refused == run && of == role && running =>
                {
                    Seen2 { seen: Seen::Refused(refusal), unread: empty }
                }
                (_, None) => Seen2 { seen: Seen::Absent, unread: empty },
                (_, Some(container)) => {
                    let seen = match container.state {
                        Docked::Created => Seen::Created,
                        Docked::Running { started } => Seen::Running { started },
                        Docked::Exited { code, oom, finished } => Seen::Exited { code, oom, finished },
                    };
                    let copied = usize::try_from(ledger.copied(run, role)).unwrap_or(usize::MAX);
                    let unread = container.log.get(copied).map_or(Unread::Nothing, Unread::Next);
                    Seen2 { seen, unread }
                }
            }
        }
    };
    let facts = Facts {
        record: &session.record,
        ledger,
        container,
        attachment: &session.memory.attachment,
        served: world.repository == Repository::Served,
        now: world.now,
        limits: &bounds.limits,
    };

    reconcile(&facts)
}

fn may(world: &World, bounds: &Bounds, fault: Fault) -> bool {
    let allowed = bounds.kinds & fault.bit() != 0;
    let unused = world.spent.kinds & fault.bit() == 0;

    allowed && unused && world.spent.faults < bounds.faults
}

/// Every action that can happen in `world`.
pub(super) fn enabled(world: &World, bounds: &Bounds) -> Vec<Action> {
    let mut actions = Vec::new();
    for (index, session) in world.sessions.iter().enumerate() {
        let at = u8::try_from(index).unwrap_or(u8::MAX);
        if session.truth.said < bounds.lines {
            actions.push(Action::Say { session: at, line: session.truth.said + 1 });
        }
        if session.truth.said > 0 {
            actions.push(Action::Say { session: at, line: session.truth.said });
        }
        let unasked = bounds.asks & !session.truth.asked;
        for (ask, bit) in
            [(Asked::Hibernate, HIBERNATE), (Asked::Wake, WAKE), (Asked::Stop, STOP), (Asked::Archive, ARCHIVE)]
        {
            if unasked & bit != 0 {
                actions.push(Action::Ask { session: at, ask });
            }
        }
        if unasked & INTERRUPT != 0 {
            actions.push(Action::Interrupt { session: at });
        }
        let effect = effect(world, index, bounds);
        if !matches!(effect, Effect::Wait(_)) {
            actions.push(Action::Hub { session: at });
        }
        if matches!(effect, Effect::Stdin(_)) && world.spent.hub_crashes < bounds.hub_crashes {
            actions.push(Action::Torn { session: at });
        }
        if matches!(effect, Effect::Docker(Ask::Start { .. })) && may(world, bounds, Fault::StartRefused) {
            actions.push(Action::Fault { session: at, fault: Fault::StartRefused });
        }
        for (&(_, role), container) in &session.containers {
            if !container.running() {
                continue;
            }
            match role {
                Role::Claude => {
                    let tasks = world.spent.tasks < bounds.tasks;
                    actions.extend(
                        claude::steps(container, tasks).into_iter().map(|step| Action::Claude { session: at, step }),
                    );
                    if matches!(container.process, Process::Booting | Process::Live(_)) {
                        for fault in [Fault::ClaudeCrash, Fault::ClaudeOutOfMemory] {
                            if may(world, bounds, fault) {
                                actions.push(Action::Fault { session: at, fault });
                            }
                        }
                    }
                }
                Role::Bootstrap => {
                    if matches!(container.process, Process::Bootstrapping) {
                        actions.push(Action::Bootstrapped { session: at });
                        for fault in [Fault::BootstrapFailed, Fault::BootstrapSlow, Fault::BootstrapOutOfMemory] {
                            if may(world, bounds, fault) {
                                actions.push(Action::Fault { session: at, fault });
                            }
                        }
                    }
                }
            }
        }
        let attached = matches!(session.memory.attachment, Attachment::Open { .. });
        if attached && may(world, bounds, Fault::Drop) {
            actions.push(Action::Fault { session: at, fault: Fault::Drop });
        }
        if !session.containers.is_empty() && may(world, bounds, Fault::Remove) {
            actions.push(Action::Fault { session: at, fault: Fault::Remove });
        }
        let claude_runs =
            session.containers.iter().any(|((_, role), container)| *role == Role::Claude && container.running());
        if !claude_runs && !session.home.saved.is_empty() && may(world, bounds, Fault::Wipe) {
            actions.push(Action::Fault { session: at, fault: Fault::Wipe });
        }
    }
    if world.spent.hub_crashes < bounds.hub_crashes {
        actions.push(Action::Crash { outage: false });
        actions.push(Action::Crash { outage: true });
    }
    let running = world.sessions.iter().any(|session| session.containers.values().any(Container::running));
    if running && may(world, bounds, Fault::Daemon) {
        actions.push(Action::Fault { session: 0, fault: Fault::Daemon });
    }
    if world.image == Image::Present && may(world, bounds, Fault::NoImage) {
        actions.push(Action::Fault { session: 0, fault: Fault::NoImage });
    }
    if world.repository == Repository::Served && may(world, bounds, Fault::Unserve) {
        actions.push(Action::Fault { session: 0, fault: Fault::Unserve });
    }
    let hub_waits = !actions.iter().any(|action| matches!(action, Action::Hub { .. }));
    if hub_waits {
        actions.push(Action::Tick);
    }

    actions
}

/// Whether one more session than session `index` may be awake.
fn room(world: &World, index: usize, bounds: &Bounds) -> Room {
    let awake = world
        .sessions
        .iter()
        .enumerate()
        .filter(|(other, session)| *other != index && counts(&session.record, session.ledger()))
        .count();

    if awake < bounds.cap {
        Room::Free
    } else {
        Room::Full
    }
}

/// What `action` does to `world`.
pub(super) fn apply(world: &World, action: Action, bounds: &Bounds) -> Outcome {
    let mut next = world.clone();
    let applied = act(&mut next, action, bounds);
    match applied {
        Ok(Applied::Done) => Outcome::Next(next),
        Ok(Applied::Pruned) => Outcome::Pruned,
        Err(why) => Outcome::Violation(why),
    }
}

fn act(world: &mut World, action: Action, bounds: &Bounds) -> Result<Applied, String> {
    let now = world.now;
    match action {
        Action::Say { session, line } => {
            let index = usize::from(session);
            let room = room(world, index, bounds);
            let session = &mut world.sessions[index];
            let answered = answer(Asked::Say(u128::from(line)), &session.record, session.ledger(), room, now);
            if let Answer::Append(entry) = answered {
                session.append(entry);
                session.truth.said = session.truth.said.max(line);
                session.truth.armed = true;
            }
        }
        Action::Ask { session, ask } => {
            let index = usize::from(session);
            let room = room(world, index, bounds);
            let session = &mut world.sessions[index];
            let bit = match ask {
                Asked::Hibernate => HIBERNATE,
                Asked::Wake => WAKE,
                Asked::Stop => STOP,
                Asked::Archive => ARCHIVE,
                Asked::Say(_) => 0,
            };
            session.truth.asked |= bit;
            if let Answer::Record(record) = answer(ask, &session.record, session.ledger(), room, now) {
                session.truth.armed = matches!(ask, Asked::Wake);
                session.record = record;
            }
        }
        Action::Interrupt { session } => {
            let session = &mut world.sessions[usize::from(session)];
            session.truth.asked |= INTERRUPT;
            if let Ok(write) = interrupt(&session.record, session.ledger(), &session.memory.attachment) {
                return stdin(session, write);
            }
        }
        Action::Hub { session } => {
            let index = usize::from(session);
            let effect = effect(world, index, bounds);
            let image = world.image;
            return perform(&mut world.sessions[index], effect, now, image, bounds);
        }
        Action::Torn { session } => {
            let index = usize::from(session);
            let run = world.sessions[index].record.run;
            if let Some(container) = world.sessions[index].containers.get_mut(&(run, Role::Claude)) {
                container.pipe.push(Piped::Fragment);
            }
            crash(world);
        }
        Action::Crash { outage } => {
            crash(world);
            if outage {
                world.now = now + Duration::from_nanos(bounds.longest());
            }
        }
        Action::Claude { session, step } => {
            if step == Step::Task {
                world.spent.tasks += 1;
            }
            return claude::take(&mut world.sessions[usize::from(session)], step, now).map(|()| Applied::Done);
        }
        Action::Bootstrapped { session } => {
            let session = &mut world.sessions[usize::from(session)];
            if let Some(key) = running(session, Role::Bootstrap) {
                end(session, key, Cause::Bootstrap, (0, false), now);
            }
        }
        Action::Fault { session, fault } => {
            world.spent.faults += 1;
            world.spent.kinds |= fault.bit();
            return Ok(fail(world, usize::from(session), fault, now));
        }
        Action::Tick => world.now = now + Duration::from_nanos(1),
    }

    Ok(Applied::Done)
}

fn crash(world: &mut World) {
    world.spent.hub_crashes += 1;
    for session in &mut world.sessions {
        session.memory = super::world::Memory::new();
    }
}

/// The key of a session's running container of `role`.
fn running(session: &Session, role: Role) -> Option<(u32, Role)> {
    session.containers.iter().find(|((_, of), container)| *of == role && container.running()).map(|(key, _)| *key)
}

fn fail(world: &mut World, index: usize, fault: Fault, now: Time) -> Applied {
    match fault {
        Fault::Daemon => {
            for session in &mut world.sessions {
                let keys: Vec<(u32, Role)> = session
                    .containers
                    .iter()
                    .filter(|(_, container)| container.running())
                    .map(|(key, _)| *key)
                    .collect();
                for key in keys {
                    end(session, key, Cause::Daemon, (143, false), now);
                }
            }
        }
        Fault::NoImage => world.image = Image::Missing,
        Fault::Unserve => world.repository = Repository::Unserved,
        Fault::Drop => world.sessions[index].memory.attachment = Attachment::Detached,
        Fault::Wipe => world.sessions[index].home = super::world::Home::default(),
        Fault::Remove => remove(&mut world.sessions[index]),
        Fault::StartRefused => {
            let session = &mut world.sessions[index];
            if let Some((role, _)) = session.record.phase.container() {
                let run = session.record.run;
                session.memory.refused = Refused::Refused { run, role, refusal: Refusal::Start };
                session.truth.armed = false;
            }
        }
        Fault::ClaudeCrash | Fault::ClaudeOutOfMemory => {
            let session = &mut world.sessions[index];
            if let Some(key) = running(session, Role::Claude) {
                let exit = if fault == Fault::ClaudeCrash {
                    (1, false)
                } else {
                    (137, true)
                };
                let cause = if fault == Fault::ClaudeCrash {
                    Cause::Crash
                } else {
                    Cause::OutOfMemory
                };
                end(session, key, cause, exit, now);
            }
        }
        Fault::BootstrapFailed | Fault::BootstrapSlow | Fault::BootstrapOutOfMemory => {
            let session = &mut world.sessions[index];
            if let Some(key) = running(session, Role::Bootstrap) {
                let exit = match fault {
                    Fault::BootstrapFailed => (78, false),
                    Fault::BootstrapSlow => (75, false),
                    _ => (137, true),
                };
                end(session, key, Cause::Bootstrap, exit, now);
            }
        }
    }

    Applied::Done
}

/// A session's container removed by hand, its unread output with it.
fn remove(session: &mut Session) {
    let Some((&key, container)) = session.containers.iter().next() else {
        return;
    };
    let ledger = session.ledger().clone();
    let copied = usize::try_from(ledger.copied(key.0, key.1)).unwrap_or(usize::MAX);
    let unread: Vec<LineId> = container
        .log
        .iter()
        .skip(copied)
        .filter_map(|output| match output.event {
            Event::Receipt(line) => Some(line),
            _ => None,
        })
        .collect();
    let cut = match &container.process {
        Process::Live(claude) => claude::cut(claude),
        Process::Made | Process::Booting | Process::Bootstrapping | Process::Dead => false,
    };
    let named = session.named() == Some(key) && matches!(session.record.phase.container(), Some((_, Run::Running)));
    session.truth.unknowable.extend(unread);
    if cut {
        session.truth.cut.insert(key.0);
    }
    if named {
        session.truth.armed = false;
    }
    if matches!(session.memory.attachment, Attachment::Open { run, .. } if run == key.0) {
        session.memory.attachment = Attachment::Detached;
    }
    session.containers.remove(&key);
}

/// A container exits.
pub(super) fn end(session: &mut Session, key: (u32, Role), cause: Cause, exit: (i32, bool), now: Time) {
    let state = match session.record.phase.container() {
        Some((role, state)) if session.named() == Some(key) && role == key.1 => state,
        _ => Run::Exited,
    };
    let Some(container) = session.containers.get_mut(&key) else {
        return;
    };
    let (code, oom) = exit;
    container.state = Docked::Exited { code, oom, finished: now };
    if let Process::Live(claude) = &container.process
        && claude::cut(claude)
    {
        session.truth.cut.insert(key.0);
    }
    container.process = Process::Dead;
    container.pipe.clear();
    container.ended = Ended::Ended { cause, state };
    if matches!(session.memory.attachment, Attachment::Open { run, .. } if run == key.0) && key.1 == Role::Claude {
        session.memory.attachment = Attachment::Detached;
    }
    let ready = key.1 == Role::Bootstrap && code == 0 && !oom;
    let failed = (state == Run::Running && !ready) || matches!(state, Run::Stopping(Why::Overdue(_)));
    if failed {
        session.truth.armed = false;
    }
}

/// The hub does `effect` for `session`.
fn perform(session: &mut Session, effect: Effect, now: Time, image: Image, bounds: &Bounds) -> Result<Applied, String> {
    match effect {
        Effect::Record(record) => write(session, record, bounds),
        Effect::Append(entry) => {
            let attached = match entry.what {
                What::Attached { run } => Some(run),
                _ => None,
            };
            session.append(entry);
            if let Some(run) = attached {
                let marker = u64::try_from(session.transcript.len()).unwrap_or(u64::MAX);
                session.memory.attachment = Attachment::Marked { run, marker };
            }
            Ok(Applied::Done)
        }
        Effect::Docker(ask) => docker(session, ask, now, image),
        Effect::Stdin(write) => stdin(session, write),
        Effect::Wait(_) => Ok(Applied::Done),
    }
}

/// The hub writes `record`.
fn write(session: &mut Session, record: Record, bounds: &Bounds) -> Result<Applied, String> {
    let old = session.record;
    if record.run > old.run {
        let reattached = matches!(old.last, Last::Ended { how: How::Stopped(Why::Reattach), .. });
        let first = old.last == Last::Unrun;
        let allowed = session.truth.armed || reattached || first;
        if !allowed {
            return Err(format!(
                "run {} started with no wake and no line since run {} ended {:?}",
                record.run, old.run, old.last
            ));
        }
        session.truth.armed = false;
        if record.run > bounds.runs {
            return Ok(Applied::Pruned);
        }
    }
    if record.last != old.last
        && let Last::Ended { run, how } = record.last
    {
        let ended = record
            .phase
            .container()
            .and_then(|(role, _)| session.containers.get(&(run, role)))
            .map(|container| container.ended);
        if let Some(Ended::Ended { cause, state }) = ended {
            if how == How::Lost && cause == Cause::Asked {
                return Err(format!("run {run}'s container, which the hub stopped, was read as Lost"));
            }
            let stopped = matches!(how, How::Stopped(_));
            if stopped && cause != Cause::Asked && state == Run::Running {
                return Err(format!("run {run}'s container, which ended by itself ({cause:?}), was read as {how:?}"));
            }
        }
        if how.fails() && record.phase == Phase::Off {
            session.truth.armed = false;
        }
    }
    let slept = old.desired == Desired::Awake && record.desired != Desired::Awake;
    if slept && record.last == old.last {
        session.truth.armed = false;
    }
    session.record = record;
    if let Refused::Refused { run, role, .. } = session.memory.refused {
        let still = record.run == run && record.phase.container() == Some((role, Run::Running));
        if !still {
            session.memory.refused = Refused::Nothing;
        }
    }

    Ok(Applied::Done)
}

fn docker(session: &mut Session, ask: Ask, now: Time, image: Image) -> Result<Applied, String> {
    match ask {
        Ask::Create { run, role, conversation } => {
            if session.containers.contains_key(&(run, role)) {
                return Ok(Applied::Done);
            }
            if image == Image::Missing {
                session.memory.refused = Refused::Refused { run, role, refusal: Refusal::NoImage };
                session.truth.armed = false;
                return Ok(Applied::Done);
            }
            session.containers.insert((run, role), Container::made(conversation));
        }
        Ask::Start { run, role } => {
            if let Some(container) = session.containers.get_mut(&(run, role))
                && container.state == Docked::Created
            {
                container.state = Docked::Running { started: now };
                container.process = match role {
                    Role::Claude => Process::Booting,
                    Role::Bootstrap => Process::Bootstrapping,
                };
            }
        }
        Ask::Stop { run, role } => {
            let running = session.containers.get(&(run, role)).is_some_and(Container::running);
            if running {
                if let Some(container) = session.containers.get_mut(&(run, role)) {
                    container.asked_stop = true;
                }
                end(session, (run, role), Cause::Asked, (143, false), now);
            }
        }
        Ask::Remove { run, role } => {
            let Some(container) = session.containers.get(&(run, role)) else {
                return Ok(Applied::Done);
            };
            if container.running() {
                return Ok(Applied::Done);
            }
            let copied = usize::try_from(session.ledger().copied(run, role)).unwrap_or(usize::MAX);
            if copied < container.log.len() {
                return Err(format!(
                    "run {run}'s {role:?} container was removed before its output was in the transcript"
                ));
            }
            session.containers.remove(&(run, role));
        }
        Ask::Attach { run } => {
            let running = session.containers.get(&(run, Role::Claude)).is_some_and(Container::running);
            if running {
                let marker = match session.memory.attachment {
                    Attachment::Marked { run: marked, marker } if marked == run => marker,
                    _ => 0,
                };
                session.memory.attachment = Attachment::Open { run, marker, wrote: Vec::new() };
            }
        }
    }

    Ok(Applied::Done)
}

/// The hub writes `write` to the stdin it is attached to.
fn stdin(session: &mut Session, write: Write) -> Result<Applied, String> {
    let ledger: Ledger = session.ledger().clone();
    let run = session.record.run;
    let Attachment::Open { run: open, wrote, .. } = &mut session.memory.attachment else {
        return Ok(Applied::Done);
    };
    let Some(container) = session.containers.get_mut(&(*open, Role::Claude)) else {
        return Ok(Applied::Done);
    };
    if *open != run || !container.running() {
        return Ok(Applied::Done);
    }
    if let Write::Line(line) = write
        && ledger.receipted(line)
    {
        return Err(format!("{line:?} was written to stdin after its receipt was in the transcript"));
    }
    let piped = match write {
        Write::Line(line) => Piped::Line(line),
        Write::Interrupt => Piped::Interrupt,
    };
    container.pipe.push(piped);
    wrote.push(write);

    Ok(Applied::Done)
}
