//! A session's container through its life (ADR-0057): its volumes made and the credential read, made from the
//! settings, started, attached and followed, and stopped and removed once it is idle, archived, or has ended.

use super::asks::{Archived, Opened, Refusal};
use super::container::{Made, Unmade, spec};
use super::credential::{Crediting, Provisioned};
use super::output::Reading;
use super::record::Conversation;
use super::session::{Activity, Flow, Life, Running, Stdin, Step, Then};
use super::streams::user_line;
use super::transcript::{Ended, Entry, Note, Turning};
use super::{Act, Acting, Acts, Asking, Does, Feed, Sessions};
use crate::{
    Acted, Attach, ContainerId, ContainerState, Create, Created, DockerError, Inspect, Inspected, Logs, Remove, Start,
    Stop, Until, Unwritable,
};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Turn, path};
use aethereal_mcp::{Admit, Dismiss, Mcp};
use aethereal_tools::Caller;
use core::mem;
use std::collections::BTreeSet;

impl Sessions {
    /// Starts a container of `session` for its run `run`, which its life is already `Starting` for at its first step:
    /// its volumes made first, and the credential read.
    pub(super) fn start<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, run: u32) -> Result<(), Fault> {
        self.house(turn, session, run, 0)?;
        self.dirty(session);

        Ok(())
    }

    /// Attaches to and follows `container`, of `session`'s run `run`, which runs: its logs from the start, skipping
    /// what the transcript holds.
    pub(super) fn resume<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let Some(docker) = self.docker() else {
            return Ok(());
        };
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let lines = match mem::replace(&mut held.life, Life::Stopped) {
            Life::Starting { lines, .. } => lines,
            Life::Running(_) | Life::Stopping { .. } | Life::Stopped | Life::Archived => Vec::new(),
        };
        let feed = Feed { session, run };
        let attach = self.attaching.open(turn, docker.protocol(), Attach { container: container.clone() }, feed)?;
        let logs = Logs { container: container.clone(), since_nanos: 0, until: Until::Stopped };
        let logs = self.logging.open(turn, docker.protocol(), logs, feed)?;
        let reading = Reading::skipping(held.transcript.output_bytes());
        let turning = held.transcript.turning();
        let has_lines = !lines.is_empty();
        let is_busy = has_lines || turning == Turning::Running;
        held.life = Life::Running(Box::new(Running {
            container,
            run,
            stdin: Stdin::Attaching { lines: lines.iter().map(|text| user_line(text)).collect() },
            activity: Activity::Working,
            tasks: BTreeSet::new(),
            reading,
            logs: Flow::Open(logs),
            attach: Flow::Open(attach),
        }));
        if !is_busy {
            self.settle(turn, session)?;
        }
        self.dirty(session);

        Ok(())
    }

    /// Stops the container of `session`, which runs, noting why, and leaves the session as `then` says once it is
    /// removed.
    pub(super) fn stop<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        why: Ended,
        then: Then,
    ) -> Result<(), Fault> {
        let due = self.due(turn, self.settings.grace_secs);
        let grace_secs = self.settings.grace_secs;
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = mem::replace(&mut held.life, Life::Stopped) else {
            return Ok(());
        };
        let Running { container, run, activity, .. } = *running;
        activity.cancel(turn);
        let stop = Stop { container: container.clone(), grace_secs };
        turn.to(path::<crate::Docker>(())).within(due).ask(&stop, &Acting { session, run, act: Act::Stop })?;
        held.life = Life::Stopping { container, run, then };
        self.note(session, Entry::Hub(Note::Stopped { run, why }));

        Ok(())
    }

    /// The container of `session`'s run `run` has ended by itself, as `why` says: it is removed, and the session
    /// stopped, to be resumed. Nothing is done when the session's container is not that run's, or is being stopped.
    pub(super) fn ended<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        why: Ended,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let is_current = matches!(&held.life, Life::Running(running) if running.run == run);
        if !is_current {
            return Ok(());
        }
        let Life::Running(running) = mem::replace(&mut held.life, Life::Stopped) else {
            return Ok(());
        };
        running.activity.cancel(turn);
        let container = running.container;
        held.life = Life::Stopping { container: container.clone(), run, then: Then::Rest { lines: Vec::new() } };
        self.note(session, Entry::Hub(Note::Stopped { run, why }));

        self.remove(turn, session, container, run)
    }

    /// Asks Docker to remove `container`, of `session`'s run `run`.
    pub(super) fn remove<S: Does>(
        &self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let due = self.due(turn, self.settings.grace_secs);
        turn.to(path::<crate::Docker>(()))
            .within(due)
            .ask(&Remove { container }, &Acting { session, run, act: Act::Remove })?;

        Ok(())
    }

    /// The start of `session`'s run `run` failed, as `why` says: the session is stopped, and each who waits on it
    /// answered.
    pub(super) fn unstarted<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        why: &Refusal,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Starting { opening, .. } = mem::replace(&mut held.life, Life::Stopped) else {
            return Ok(());
        };
        for waiting in opening {
            turn.answer(waiting, &Opened::Refused(why.clone()))?;
        }
        self.note(session, Entry::Hub(Note::Unstarted { run, why: format!("{why:?}") }));

        Ok(())
    }

    /// Whether `session` is starting its run `run` at `step`.
    pub(super) fn is_at(&self, session: u32, run: u32, at: fn(&Step) -> bool) -> bool {
        let Some(held) = self.held.get(&session) else {
            return false;
        };

        matches!(&held.life, Life::Starting { run: starting, step, .. } if *starting == run && at(step))
    }
}

impl Handles<Answer<Provisioned, Crediting>> for Sessions {
    type Sends = Acts;

    /// The credential is read, a bearer drawn, and the drop box made: `Mcp` is told the bearer is the session's, and
    /// Docker asked to make the session's container, which is given both.
    fn handle(
        &mut self,
        (answered, Crediting { session, run }): (Answered<Provisioned>, Crediting),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let is_crediting = self.is_at(session, run, |step| matches!(step, Step::Crediting));
        if !is_crediting {
            return Ok(());
        }
        let (env, bearer, drop_box) = match answered {
            Answered::Yes(Provisioned::Ready { env, bearer, drop_box }) => (env, bearer, drop_box),
            Answered::Yes(Provisioned::Undrawn) => return self.unstarted(turn, session, run, &Refusal::NoBearer),
            Answered::Yes(Provisioned::Unboxed) => return self.unstarted(turn, session, run, &Refusal::NoDropBox),
            Answered::Yes(Provisioned::Unread) | Answered::No(_) => {
                return self.unstarted(turn, session, run, &Refusal::NoCredential);
            }
        };
        turn.to(path::<Mcp>(())).send(&Admit { bearer, caller: Caller::Session { session } })?;
        let conversation = self.held.get(&session).map_or(Conversation::Unbegun, |held| held.conversation.clone());
        let made = Made { session, run, conversation: &conversation, env, bearer, drop_box };
        let spec = match spec(&self.settings, made) {
            Ok(spec) => spec,
            Err(Unmade::Unlabelled(_)) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(DockerError::Malformed));
            }
            Err(Unmade::Unnamed) => {
                let why = Refusal::Docker(DockerError::Unwritable(Unwritable::Volume));
                return self.unstarted(turn, session, run, &why);
            }
        };
        let due = self.due(turn, 0);
        turn.to(path::<crate::Docker>(())).within(due).ask(&Create { spec }, &Asking { session, run })?;
        self.step_to(session, Step::Creating);

        self.flush(turn)
    }
}

impl Handles<Answer<Created, Asking>> for Sessions {
    type Sends = Acts;

    /// Docker made the container: it is recorded, and started. One made for a start given up on is removed.
    fn handle(
        &mut self,
        (answered, Asking { session, run }): (Answered<Created>, Asking),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let is_creating = self.is_at(session, run, |step| matches!(step, Step::Creating));
        let container = match answered {
            Answered::Yes(Created::Made(container)) => container,
            Answered::Yes(Created::Failed(why)) => return self.unstarted(turn, session, run, &Refusal::Docker(why)),
            Answered::No(why) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(DockerError::Unanswered(why)));
            }
        };
        if !is_creating {
            return self.remove(turn, session, container, run);
        }
        let due = self.due(turn, 0);
        let start = Start { container: container.clone() };
        turn.to(path::<crate::Docker>(())).within(due).ask(&start, &Acting { session, run, act: Act::Start })?;
        self.step_to(session, Step::Launching { container });
        self.dirty(session);

        self.flush(turn)
    }
}

impl Handles<Answer<Acted, Acting>> for Sessions {
    type Sends = Acts;

    /// A start, a stop, or a removal Docker did, or why not.
    fn handle(
        &mut self,
        (answered, Acting { session, run, act }): (Answered<Acted>, Acting),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let acted = match answered {
            Answered::Yes(acted) => acted,
            Answered::No(why) => Acted::Failed(DockerError::Unanswered(why)),
        };
        match act {
            Act::Start => self.started(turn, session, run, acted)?,
            Act::Stop => self.stopped(turn, session, run, &acted)?,
            Act::Remove => self.removed(turn, session, run, acted)?,
        }

        self.flush(turn)
    }
}

impl Sessions {
    /// Docker started, or did not start, the container of `session`'s run `run`.
    fn started<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        acted: Acted,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        let Life::Starting { run: starting, step: Step::Launching { container }, .. } = &held.life else {
            return Ok(());
        };
        if *starting != run {
            return Ok(());
        }
        let container = container.clone();
        if let Acted::Failed(why) = acted {
            self.remove(turn, session, container, run)?;
            return self.unstarted(turn, session, run, &Refusal::Docker(why));
        }
        let resumed = matches!(held.conversation, Conversation::Begun(_));
        self.note(session, Entry::Hub(Note::Started { run, resumed }));
        let opening = match self.held.get_mut(&session).map(|held| &mut held.life) {
            Some(Life::Starting { opening, .. }) => mem::take(opening),
            _ => Vec::new(),
        };
        self.resume(turn, session, container, run)?;
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        let listed = held.listed(session);
        for waiting in opening {
            turn.answer(waiting, &Opened::Open(listed.clone()))?;
        }

        Ok(())
    }

    /// Docker stopped the container of `session`'s run `run`, or did not: it is removed either way.
    fn stopped<S: Does>(
        &self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        acted: &Acted,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        let Life::Stopping { container, run: stopping, .. } = &held.life else {
            return Ok(());
        };
        if *stopping != run {
            return Ok(());
        }
        if let Acted::Failed(why) = acted {
            turn.log(Level::Warn, format_args!("session {session}'s container did not stop, and is removed: {why:?}"));
        }

        self.remove(turn, session, container.clone(), run)
    }

    /// Docker removed the container of `session`'s run `run`, or did not: the session is left as its stop said, and
    /// `Mcp` lets go of its bearer, so nothing calls the hub's tools as the session until its next container.
    fn removed<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        acted: Acted,
    ) -> Result<(), Fault> {
        let gone = match acted {
            Acted::Done | Acted::Unchanged | Acted::Failed(DockerError::NotFound { .. }) => Ok(()),
            Acted::Failed(why) => Err(why),
        };
        if let Err(why) = &gone {
            turn.log(Level::Warn, format_args!("session {session}'s container of run {run} was not removed: {why:?}"));
        }
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let is_stopping = matches!(&held.life, Life::Stopping { run: stopping, .. } if *stopping == run);
        if !is_stopping {
            return Ok(());
        }
        let Life::Stopping { then, .. } = mem::replace(&mut held.life, Life::Stopped) else {
            return Ok(());
        };
        turn.to(path::<Mcp>(())).send(&Dismiss { caller: Caller::Session { session } })?;
        self.chats.forget(session);
        self.dirty(session);
        match then {
            Then::Rest { lines } => {
                if !lines.is_empty() {
                    self.restart(turn, session, lines)?;
                }
            }
            Then::Archive { archiving } => {
                let archived = match gone {
                    Ok(()) => {
                        if let Some(held) = self.held.get_mut(&session) {
                            held.life = Life::Archived;
                        }
                        Archived::Done
                    }
                    Err(why) => Archived::Refused(Refusal::Docker(why)),
                };
                for waiting in archiving {
                    turn.answer(waiting, &archived)?;
                }
            }
        }

        Ok(())
    }

    /// Resumes `session`, which is stopped, in a new container, writing `lines` to it once it is attached.
    pub(super) fn restart<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        lines: Vec<String>,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        held.runs += 1;
        let run = held.runs;
        held.life = Life::Starting { run, step: Step::Housing { made: 0 }, opening: Vec::new(), lines };

        self.start(turn, session, run)
    }
}

impl Handles<Answer<Inspected, Asking>> for Sessions {
    type Sends = Acts;

    /// The state of a container whose logs or attach ended: one still running is followed again, and one that is not
    /// has ended.
    fn handle(
        &mut self,
        (answered, Asking { session, run }): (Answered<Inspected>, Asking),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let why = match answered {
            Answered::Yes(Inspected::State(ContainerState::Running)) => return self.reopen(turn, session, run),
            Answered::Yes(Inspected::State(
                ContainerState::Exited { code, oom_killed } | ContainerState::Dead { code, oom_killed },
            )) => {
                if oom_killed {
                    Ended::OutOfMemory
                } else {
                    Ended::Exited { code }
                }
            }
            Answered::Yes(Inspected::State(_) | Inspected::Failed(_)) | Answered::No(_) => Ended::Lost,
        };
        self.ended(turn, session, run, why)?;

        self.flush(turn)
    }
}

impl Sessions {
    /// Opens again the streams of `session`'s run `run` that ended while its container still runs.
    fn reopen<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, run: u32) -> Result<(), Fault> {
        let Some(docker) = self.docker() else {
            return Ok(());
        };
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = &mut held.life else {
            return Ok(());
        };
        if running.run != run {
            return Ok(());
        }
        let feed = Feed { session, run };
        if running.attach == Flow::Closed {
            let attach = Attach { container: running.container.clone() };
            running.attach = Flow::Open(self.attaching.open(turn, docker.protocol(), attach, feed)?);
        }
        if running.logs == Flow::Closed {
            let logs = Logs { container: running.container.clone(), since_nanos: 0, until: Until::Stopped };
            running.reading = Reading::skipping(held.transcript.output_bytes());
            running.logs = Flow::Open(self.logging.open(turn, docker.protocol(), logs, feed)?);
        }

        Ok(())
    }

    /// Asks Docker for the state of `session`'s container of run `run`, whose logs or attach ended.
    pub(super) fn inspect<S: Does>(
        &self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let due = self.due(turn, 0);
        turn.to(path::<crate::Docker>(())).within(due).ask(&Inspect { container }, &Asking { session, run })?;

        Ok(())
    }
}

impl Activity {
    /// Cancels the wait it holds on the timer, if it holds one.
    pub(super) fn cancel<S: Does>(&self, turn: &mut Turn<'_, Sessions, S>) {
        match self {
            Self::Idle { sleep: job, .. }
            | Self::Noticed { deadline: job, .. }
            | Self::Compacting { deadline: job, .. } => {
                turn.cancel(*job);
            }
            Self::Working | Self::Waiting => {}
        }
    }
}
