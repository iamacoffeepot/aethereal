//! A session's container through its life (ADR-0057): its record saying it starts written, its volumes made and the
//! credential read, made from the settings, started, attached and followed, and stopped and removed once it hibernates,
//! is archived, or has ended. A session whose container ended without the hub ending it hibernates with its container
//! lost, and shows so until it wakes; one whose container's start script failed its bootstrap, before Claude Code
//! started, did not start, and hibernates with its container removed.

use super::asked::Asker;
use super::asks::{Archived, Opened, Refusal};
use super::container::{Made, Unmade, spec};
use super::credential::{Crediting, Provisioned};
use super::output::Reading;
use super::record::Conversation;
use super::session::{Activity, Flow, Gone, Life, Running, Stdin, Step, Then};
use super::settings::BOOTSTRAP_FAILED;
use super::streams::user_line;
use super::transcript::{Counted, Ended, Entry, Note, Turning};
use super::{Act, Acting, Asking, Feed, Sessions};
use crate::{
    Acted, Attach, ContainerId, ContainerState, Create, Created, DockerError, Inspect, Inspected, Logs, Remove, Start,
    Stop, Until, Unwritable,
};
use aethereal_actor::{Answered, Fault, Level, Turn, path};
use core::mem;
use std::collections::BTreeSet;

impl Sessions {
    /// Starts a container of `session`, whose life is already `Starting` its run at its first step: its record, saying
    /// so, is written first, and once that lands, its volumes are made and the credential read.
    pub(super) fn start(&mut self, session: u32) {
        self.dirty(session);
    }

    /// Attaches to and follows `container`, of `session`'s run `run`, which runs: its logs from the start, skipping
    /// what the transcript holds.
    pub(super) fn resume(
        &mut self,
        turn: &mut Turn<'_, Self>,
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
        let lines = match mem::replace(&mut held.life, Life::Hibernating { gone: Gone::Removed }) {
            Life::Starting { lines, .. } => lines,
            Life::Running(_) | Life::Stopping { .. } | Life::Hibernating { .. } | Life::Archived => Vec::new(),
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
    pub(super) fn stop(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        why: Ended,
        then: Then,
    ) -> Result<(), Fault> {
        let due = self.due(turn, self.settings.grace_secs);
        let grace_secs = self.settings.grace_secs;
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = mem::replace(&mut held.life, Life::Hibernating { gone: Gone::Removed }) else {
            return Ok(());
        };
        let Running { container, run, activity, .. } = *running;
        activity.cancel(turn);
        let stop = Stop { container: container.clone(), grace_secs };
        turn.to(path::<crate::Docker>(())).within(due).ask(&stop, &Acting { session, run, act: Act::Stop })?;
        held.life = Life::Stopping { container, run, then };
        self.note(turn, session, Entry::Hub(Note::Stopped { run, why }));

        Ok(())
    }

    /// The container of `session`'s run `run` has ended by itself, as `why` says: it is removed, and the session left
    /// hibernating, its container lost. One that exited with the start script's own code failed its bootstrap before
    /// Claude Code started, so it lost nothing: it is noted so, and the session left hibernating, its container
    /// removed, as a start that failed is. Nothing is done when the session's container is not that run's, or is being
    /// stopped.
    pub(super) fn ended(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32, why: Ended) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let is_current = matches!(&held.life, Life::Running(running) if running.run == run);
        if !is_current {
            return Ok(());
        }
        let (note, gone) = match why {
            Ended::Exited { code: BOOTSTRAP_FAILED } => {
                (Note::Unbootstrapped { run, code: BOOTSTRAP_FAILED }, Gone::Removed)
            }
            Ended::Hibernated | Ended::Archived | Ended::Exited { .. } | Ended::OutOfMemory | Ended::Lost => {
                (Note::Stopped { run, why }, Gone::Lost)
            }
        };
        let Life::Running(running) = mem::replace(&mut held.life, Life::Hibernating { gone }) else {
            return Ok(());
        };
        running.activity.cancel(turn);
        self.note(turn, session, Entry::Hub(note));

        self.retire(turn, session, running.container, run, gone)
    }

    /// Removes `container`, of `session`'s run `run`, which does not run: the session is stopping until Docker has
    /// removed it, so its record names the container until it is gone, and is then left hibernating, its container
    /// gone as `gone` says.
    pub(super) fn retire(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        container: ContainerId,
        run: u32,
        gone: Gone,
    ) -> Result<(), Fault> {
        if let Some(held) = self.held.get_mut(&session) {
            let then = Then::Hibernate { opening: Vec::new(), lines: Vec::new(), gone };
            held.life = Life::Stopping { container: container.clone(), run, then };
        }

        self.remove(turn, session, container, run)
    }

    /// Adopts `container`, of `session`'s run `run`, which a hub before this one started and which runs: the
    /// transcript notes the run started unless it does already, so it counts the run's output from its start, and the
    /// container is attached to and followed from its logs, skipping what the transcript holds of them.
    pub(super) fn adopt(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        if held.transcript.counted() != Counted::Run(run) {
            self.note_started(turn, session, run);
        }

        self.resume(turn, session, container, run)
    }

    /// Notes in `session`'s transcript that its container of `run` started, carrying on Claude's conversation when it
    /// has one.
    fn note_started(&mut self, turn: &Turn<'_, Self>, session: u32, run: u32) {
        let Some(held) = self.held.get(&session) else {
            return;
        };
        let resumed = held.resumes();
        self.note(turn, session, Entry::Hub(Note::Started { run, resumed }));
    }

    /// Asks Docker to remove `container`, of `session`'s run `run`.
    pub(super) fn remove(
        &self,
        turn: &mut Turn<'_, Self>,
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

    /// The start of `session`'s run `run` failed, as `why` says: the session hibernates, and each who waits on it is
    /// answered, or declined.
    pub(super) fn unstarted(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        run: u32,
        why: &Refusal,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Starting { opening, .. } = mem::replace(&mut held.life, Life::Hibernating { gone: Gone::Removed })
        else {
            return Ok(());
        };
        let refused = Opened::Refused(why.clone());
        for waiting in opening {
            self.reply(turn, waiting, &refused)?;
        }
        self.note(turn, session, Entry::Hub(Note::Unstarted { run, why: format!("{why:?}") }));

        Ok(())
    }

    /// Whether `session` is starting its run `run` at `step`.
    pub(super) fn is_at(&self, session: u32, run: u32, at: fn(&Step) -> bool) -> bool {
        let Some(held) = self.held.get(&session) else {
            return false;
        };

        matches!(&held.life, Life::Starting { run: starting, step, .. } if *starting == run && at(step))
    }

    /// The credential is read, a bearer drawn, and the drop box made: the bearer is held as the session's, and Docker
    /// asked to make the session's container, which is given both.
    pub(super) fn handle_provisioned(
        &mut self,
        (answered, Crediting { session, run }): (Answered<Provisioned>, Crediting),
        turn: &mut Turn<'_, Self>,
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
        self.bearers.admit(bearer, session);
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

    /// Docker made the container: it is recorded, and started. One made for a start given up on is removed.
    pub(super) fn handle_created(
        &mut self,
        (answered, Asking { session, run }): (Answered<Created>, Asking),
        turn: &mut Turn<'_, Self>,
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

    /// A start, a stop, or a removal Docker did, or why not.
    pub(super) fn handle_acted(
        &mut self,
        (answered, Acting { session, run, act }): (Answered<Acted>, Acting),
        turn: &mut Turn<'_, Self>,
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
    fn started(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32, acted: Acted) -> Result<(), Fault> {
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
            self.unstarted(turn, session, run, &Refusal::Docker(why))?;
            return self.retire(turn, session, container, run, Gone::Removed);
        }
        self.note_started(turn, session, run);
        let opening = match self.held.get_mut(&session).map(|held| &mut held.life) {
            Some(Life::Starting { opening, .. }) => mem::take(opening),
            _ => Vec::new(),
        };
        self.resume(turn, session, container, run)?;
        let Some(row) = self.row(session) else {
            return Ok(());
        };
        let open = Opened::Open(row);
        for waiting in opening {
            self.reply(turn, waiting, &open)?;
        }

        Ok(())
    }

    /// Docker stopped the container of `session`'s run `run`, or did not: it is removed either way.
    fn stopped(&self, turn: &mut Turn<'_, Self>, session: u32, run: u32, acted: &Acted) -> Result<(), Fault> {
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

    /// Docker removed the container of `session`'s run `run`, or did not: the session is left as its stop said,
    /// hibernating or archived, and its bearer is let go of, so nothing calls the hub's tools as the session until its
    /// next container.
    fn removed(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32, acted: Acted) -> Result<(), Fault> {
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
        let Life::Stopping { then, .. } = mem::replace(&mut held.life, Life::Hibernating { gone: Gone::Removed })
        else {
            return Ok(());
        };
        self.bearers.dismiss(session);
        self.chats.forget(session);
        self.dirty(session);
        match then {
            Then::Hibernate { opening, lines, gone: rested } => {
                if let Some(held) = self.held.get_mut(&session) {
                    held.life = Life::Hibernating { gone: rested };
                }
                let is_opened = !opening.is_empty();
                let has_lines = !lines.is_empty();
                let is_woken = is_opened || has_lines;
                if is_woken {
                    self.restart(session, opening, lines);
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
                    self.reply(turn, waiting, &archived)?;
                }
            }
        }

        Ok(())
    }

    /// Wakes `session`, which hibernates, in a new container on its home: `opening` is answered once it runs, and
    /// `lines` are written to it once it is attached.
    pub(super) fn restart(&mut self, session: u32, opening: Vec<Asker<Opened>>, lines: Vec<String>) {
        let Some(held) = self.held.get_mut(&session) else {
            return;
        };
        held.runs += 1;
        held.life = Life::Starting { run: held.runs, step: Step::Recording, opening, lines };

        self.start(session);
    }

    /// The state of a container whose logs or attach ended: one still running is followed again, and one that is not
    /// has ended.
    pub(super) fn handle_inspected(
        &mut self,
        (answered, Asking { session, run }): (Answered<Inspected>, Asking),
        turn: &mut Turn<'_, Self>,
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
    fn reopen(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32) -> Result<(), Fault> {
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
    pub(super) fn inspect(
        &self,
        turn: &mut Turn<'_, Self>,
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
    pub(super) fn cancel(&self, turn: &mut Turn<'_, Sessions>) {
        match self {
            Self::Idle { sleep: job, .. }
            | Self::Noticed { deadline: job, .. }
            | Self::Compacting { deadline: job, .. } => {
                turn.cancel(*job);
            }
            Self::Working | Self::Tasked => {}
        }
    }
}
