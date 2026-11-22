//! A run's bootstrap container (ADR-0057): made from the session image on the session's home, as the session's user,
//! its command the start script alone, which readies the session's work tree and exits. The hub follows its logs into
//! the transcript, for people to read, and once they end, as Docker ends them only once it has stopped and everything
//! it wrote is read, asks Docker how it ended. That state alone judges it: exit 0 readied the tree, and once Docker has
//! removed it, the run's Claude Code container is made; any other code, the kernel's kill, or no container at all is a
//! failed start. Nothing it writes is read for what it says. A hub that starts again finds one by its record and by
//! its label, and judges it the same way, whatever state Docker lists it in.

use super::asks::{Refusal, Unbootstrapped};
use super::output::Reading;
use super::session::{Flow, Life, Stage, Step};
use super::transcript::{Counted, Ended, Entry, Note};
use super::{Feed, Sessions};
use crate::{ContainerId, Logs, Until};
use aethereal_actor::{Fault, Turn};

/// The most lines a bootstrap container wrote to stderr that its failed start's refusal shows, the last it wrote.
const SAID_LINES: usize = 3;

impl Sessions {
    /// Follows the logs of `container`, the bootstrap container of `session`'s run `run`, which Docker started, into the
    /// transcript from its start, skipping what the transcript holds of them, until they end: the transcript notes it
    /// started unless it does already. Nothing is written to it, as its stdin is closed.
    pub(super) fn bootstrap(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let Some(docker) = self.docker() else {
            return Ok(());
        };
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        if held.transcript.counted() != Counted::Bootstrap(run) {
            self.note(turn, session, Entry::Hub(Note::Bootstrapping { run }));
        }
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        let reading = Reading::skipping(held.transcript.output_bytes());
        let logs = Logs { container: container.clone(), since_nanos: 0, until: Until::Stopped };
        let logs = Flow::Open(self.logging.open(turn, docker.protocol(), logs, Feed { session, run })?);
        self.step_to(session, Step::Bootstrapping { container, reading, logs });
        self.dirty(session);

        Ok(())
    }

    /// Adopts `container`, the bootstrap container of `session`'s run `run`, which a hub before this one made, in
    /// whatever state Docker lists it: it is followed from its logs, and judged once they end, as any bootstrap
    /// container is. Nothing waits on it: what waited on it went with the hub before.
    pub(super) fn adopt_bootstrap(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        container: ContainerId,
        run: u32,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let launching = Step::Launching { stage: Stage::Bootstrap, container: container.clone() };
        held.life = Life::Starting { run, step: launching, opening: Vec::new(), lines: Vec::new() };

        self.bootstrap(turn, session, container, run)
    }

    /// The bootstrap container of `session`'s run `run`, whose logs have ended, ended as Docker says, `ended`. One that
    /// exited 0 readied the work tree: it is removed, and Claude Code's container made once it is. Any other did not,
    /// and the run's start fails, its refusal showing how it ended and the last lines it wrote to stderr.
    pub(super) fn bootstrapped(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        run: u32,
        ended: Ended,
    ) -> Result<(), Fault> {
        let Some(Step::Bootstrapping { container, .. }) = self.step_of(session, run) else {
            return Ok(());
        };
        let container = container.clone();
        if ended == (Ended::Exited { code: 0 }) {
            self.step_to(session, Step::Clearing { container: container.clone() });
            return self.remove(turn, session, container, run);
        }
        let why = Refusal::Unbootstrapped(Unbootstrapped { ended, said: self.bootstrap_said(session, run) });

        self.unstarted(turn, session, run, &why)
    }

    /// Docker removed the bootstrap container of `session`'s run, which exited 0, or it was gone: its bearer is let go
    /// of, and the record is written again saying the run starts, so the Claude Code container made once it lands is
    /// one the record names.
    pub(super) fn cleared(&mut self, session: u32) {
        self.bearers.dismiss(session);
        self.step_to(session, Step::Recording { stage: Stage::Claude });
        self.dirty(session);
    }

    /// The bootstrap container of `session`'s run `run`, which its record names, is not among those Docker lists as a
    /// hub that starts again lists them: it is gone, and the start failed. The session hibernates saying so, holding no
    /// line, as what waited for it went with the hub before.
    pub(super) fn bootstrap_gone(&mut self, turn: &Turn<'_, Self>, session: u32, run: u32) {
        let said = self.bootstrap_said(session, run);
        let why = Refusal::Unbootstrapped(Unbootstrapped { ended: Ended::Lost, said }).to_string();
        self.left_unstarted(turn, session, run, why);
    }

    /// The last lines the bootstrap container of `session`'s run `run` wrote to stderr, as the transcript holds them,
    /// shown to people and never read: none when the transcript does not count that container's output.
    fn bootstrap_said(&self, session: u32, run: u32) -> Vec<String> {
        let Some(held) = self.held.get(&session) else {
            return Vec::new();
        };
        if held.transcript.counted() != Counted::Bootstrap(run) {
            return Vec::new();
        }

        held.transcript.stderr_tail(SAID_LINES)
    }
}
