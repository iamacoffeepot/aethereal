//! The fake Claude Code, as the probes found the real one: a line read is receipted and saved in one step, a line read
//! during a turn is queued behind it, an id already saved whose turn finished is receipted and skipped, one whose turn
//! was cut short runs again, `/compact` is receipted after its compaction and skipped once it finished, a line cut
//! short followed by anything ends the process, `--resume` of no conversation answers its typed error before any
//! receipt and exits, `--session-id` of a conversation that exists exits with no output, and an interrupt ends the turn
//! in flight.

use super::actions::end;
use super::world::{Cause, Claude, Container, Doing, Home, Piped, Process, Session, Tasks, Truth};
use crate::reconcile::{Begin, Event, LineId, Output, Role, TaskId};
use aethereal_actor::Time;

/// A step Claude Code takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Step {
    /// It starts, and checks its flags against its home.
    Boot,
    /// It reads the next item of its stdin: a line read while nothing runs begins its turn at once.
    Read,
    /// Its turn ends, and the next line queued begins its own.
    Answer,
    /// Its compaction ends, and the next line queued begins its turn.
    Compact,
    /// Its turn starts a background task.
    Task,
    /// Its background task ends.
    Done,
}

impl Step {
    /// Whether it happens with no fault: every step but starting a task, which is new work.
    pub const fn calm(self) -> bool {
        !matches!(self, Self::Task)
    }

    /// Whether it is sure to happen while it can: Claude reads its stdin and starts promptly, but a turn, a task, or a
    /// compaction may never end.
    pub const fn fair(self) -> bool {
        matches!(self, Self::Boot | Self::Read)
    }
}

/// Whether a live Claude has a line it read unanswered.
pub(super) fn cut(claude: &Claude) -> bool {
    let compact = |line: &LineId| matches!(line, LineId::Compact { .. });
    let turning = matches!(claude.doing, Doing::Turn { line, .. } if !compact(&line));

    turning || claude.queue.iter().any(|line| !compact(line))
}

/// The steps Claude can take in `container`, starting a task only when `tasks` allows.
pub(super) fn steps(container: &Container, tasks: bool) -> Vec<Step> {
    let mut steps = Vec::new();
    match &container.process {
        Process::Booting => steps.push(Step::Boot),
        Process::Live(claude) => {
            let readable = match container.pipe.as_slice() {
                [] | [Piped::Fragment] => false,
                [_, ..] => true,
            };
            if readable {
                steps.push(Step::Read);
            }
            match claude.doing {
                Doing::Idle => {}
                Doing::Turn { .. } => {
                    steps.push(Step::Answer);
                    if tasks && claude.tasks == Tasks::None {
                        steps.push(Step::Task);
                    }
                }
                Doing::Compacting { .. } => steps.push(Step::Compact),
            }
            if matches!(claude.tasks, Tasks::One { .. }) {
                steps.push(Step::Done);
            }
        }
        Process::Made | Process::Bootstrapping | Process::Dead => {}
    }

    steps
}

/// What a step works on: the live Claude, its container's log, its home, and the explorer's truth.
struct Live<'a> {
    claude: &'a mut Claude,
    log: &'a mut Vec<Output>,
    home: &'a mut Home,
    truth: &'a mut Truth,
    now: Time,
}

impl Live<'_> {
    fn emit(&mut self, event: Event) {
        self.log.push(Output { at: self.now, event });
    }

    /// Begins the turn of the first line queued, or its compaction, when nothing runs.
    fn next(&mut self) -> Result<(), String> {
        if self.claude.doing != Doing::Idle {
            return Ok(());
        }
        let Some(line) = self.claude.queue.pop_front() else {
            return Ok(());
        };
        if let LineId::Compact { .. } = line {
            self.emit(Event::Compacting);
            self.claude.doing = Doing::Compacting { line, since: self.now };
            return Ok(());
        }
        self.emit(Event::Init);
        self.claude.doing = Doing::Turn { line, since: self.now };
        let turns = self.truth.turns.entry(line).or_insert(0);
        *turns += 1;
        let twice = *turns > 1 && !self.truth.unknowable.contains(&line);
        if twice {
            return Err(format!("{line:?} began a second turn"));
        }

        Ok(())
    }

    /// Ends the turn in flight.
    fn answer(&mut self) -> Result<(), String> {
        if let Doing::Turn { line, .. } = self.claude.doing {
            self.emit(Event::Result);
            self.home.finished.insert(line);
            self.claude.doing = Doing::Idle;
        }

        self.next()
    }

    /// Reads `line`, `run` being the container's run.
    fn read(&mut self, line: LineId, run: u32) -> Result<(), String> {
        let seen = self.claude.seen.contains(&line);
        self.claude.seen.insert(line);
        if let LineId::Compact { .. } = line {
            let done = self.home.saved.contains(&line) && self.home.finished.contains(&line);
            if done {
                self.emit(Event::Receipt(line));
                return Ok(());
            }
            self.claude.queue.push_back(line);
            return self.next();
        }
        let saved = self.home.saved.contains(&line);
        self.emit(Event::Receipt(line));
        if seen {
            return Ok(());
        }
        if saved {
            if !self.home.finished.contains(&line) {
                self.claude.queue.push_back(line);
            }
            return self.next();
        }
        self.home.saved.push(line);
        save(self.truth, run, line)?;
        self.claude.queue.push_back(line);

        self.next()
    }
}

/// Claude, in `session`'s running Claude Code container, takes `step` at `now`.
pub(super) fn take(session: &mut Session, step: Step, now: Time) -> Result<(), String> {
    let Some(key) = session
        .containers
        .iter()
        .find(|((_, role), container)| *role == Role::Claude && container.running())
        .map(|(key, _)| *key)
    else {
        return Ok(());
    };
    if step == Step::Boot {
        boot(session, key, now);
        return Ok(());
    }
    let Some(container) = session.containers.get_mut(&key) else {
        return Ok(());
    };
    if step == Step::Read && container.pipe.first() == Some(&Piped::Fragment) {
        end(session, key, Cause::Torn, (1, false), now);
        return Err(String::from("a line the hub's crash cut short was followed by another write: Claude Code exited"));
    }
    let Process::Live(claude) = &mut container.process else {
        return Ok(());
    };
    let mut taking = Live { claude, log: &mut container.log, home: &mut session.home, truth: &mut session.truth, now };
    match step {
        Step::Read => match container.pipe.remove(0) {
            Piped::Line(line) => taking.read(line, key.0),
            Piped::Interrupt => taking.answer(),
            Piped::Fragment => Ok(()),
        },
        Step::Answer => taking.answer(),
        Step::Compact => {
            if let Doing::Compacting { line, .. } = taking.claude.doing {
                taking.emit(Event::Compacted);
                taking.emit(Event::Receipt(line));
                taking.home.saved.push(line);
                taking.home.finished.insert(line);
                taking.truth.saved.push(line);
                taking.claude.doing = Doing::Idle;
            }
            taking.next()
        }
        Step::Task => {
            taking.emit(Event::TaskStarted(TaskId(String::from("t"))));
            taking.claude.tasks = Tasks::One { since: now };
            Ok(())
        }
        Step::Done => {
            taking.emit(Event::TaskEnded(TaskId(String::from("t"))));
            taking.claude.tasks = Tasks::None;
            Ok(())
        }
        Step::Boot => Ok(()),
    }
}

fn boot(session: &mut Session, key: (u32, Role), now: Time) {
    let begun = !session.home.saved.is_empty();
    let Some(container) = session.containers.get_mut(&key) else {
        return;
    };
    let flag = container.begin;
    match flag {
        Begin::Fresh if begun => end(session, key, Cause::InUse, (1, false), now),
        Begin::Resume if !begun => {
            container.log.push(Output { at: now, event: Event::ResumeFailed });
            end(session, key, Cause::Unresumable, (1, false), now);
        }
        Begin::Fresh | Begin::Resume => container.process = Process::Live(Claude::default()),
    }
}

/// Checks a line Claude saves for the first time, in run `run`, and keeps it.
fn save(truth: &mut Truth, run: u32, line: LineId) -> Result<(), String> {
    let again = truth.saved.contains(&line) && !truth.unknowable.contains(&line);
    if again {
        return Err(format!("{line:?} was read a second time"));
    }
    if let LineId::Owner(number) = line {
        let skipped = (1..number).map(LineId::Owner).find(|earlier| !truth.saved.contains(earlier));
        if let Some(earlier) = skipped {
            return Err(format!("{line:?} was read before {earlier:?}"));
        }
        let unwarned = truth.cut.iter().find(|&&cut| cut < run && !truth.saved.contains(&LineId::Lost { run: cut }));
        if let Some(cut) = unwarned {
            return Err(format!(
                "{line:?} was read in run {run} before the lost line of run {cut}, which ended with a line unanswered"
            ));
        }
    }
    truth.saved.push(line);

    Ok(())
}
