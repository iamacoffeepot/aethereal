//! One session as the capability holds it: its record, its transcript, and where its container is in its life.

use super::asks::{self, Archived, Opened, State};
use super::output::Reading;
use super::record::{Conversation, Home, Record, Standing};
use super::transcript::Transcript;
use crate::ContainerId;
use aethereal_actor::{Held, JobId, Number, ProtocolRef};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::Writing;
use std::collections::BTreeSet;

/// A session.
#[derive(Bound)]
pub(super) struct Session {
    pub name: String,
    pub conversation: Conversation,
    /// How many containers it has run in.
    pub runs: u32,
    pub home: Home,
    pub transcript: Transcript,
    pub life: Life,
}

/// Where a session's container is in its life.
#[derive(Bound)]
pub(super) enum Life {
    /// Its container of `run` is being made and started: `step` says how far. `opening` waits to be answered once it
    /// runs, and `lines` wait to be written to it once its stdin is attached.
    Starting { run: u32, step: Step, opening: Vec<Held<Opened>>, lines: Vec<String> },
    /// Its container runs.
    Running(Box<Running>),
    /// Its container of `run` is being stopped, then removed, and then the session is left as `then` says.
    Stopping { container: ContainerId, run: u32, then: Then },
    /// No container of it runs: the next line resumes it.
    Stopped,
    /// It was archived, and never runs again.
    Archived,
}

/// How far a container being started is. A session whose home is not its user's yet goes through every step; one
/// whose home is starts at reading the credential.
#[derive(Bound)]
pub(super) enum Step {
    /// Docker makes its home's volume.
    Housing,
    /// Docker makes the container that gives the home to the session's user.
    Owning,
    /// Docker starts that container, `owner`.
    Handing { owner: ContainerId },
    /// Docker waits for `owner` to end.
    Waiting { owner: ContainerId },
    /// The credential is read.
    Crediting,
    /// Docker makes it.
    Creating,
    /// Docker starts it.
    Launching { container: ContainerId },
}

impl Step {
    /// Where a start begins, for a session whose home is as `home` says.
    pub const fn first(home: Home) -> Self {
        match home {
            Home::Unowned => Self::Housing,
            Home::Owned => Self::Crediting,
        }
    }
}

/// What a session is left as once its container is stopped and removed.
#[derive(Bound)]
pub(super) enum Then {
    /// Stopped, to be resumed: it was idle, or its container ended by itself. `lines` said meanwhile resume it once
    /// its container is removed.
    Rest { lines: Vec<String> },
    /// Archived, `archiving` answered once it is.
    Archive { archiving: Vec<Held<Archived>> },
}

/// A session whose container runs.
#[derive(Bound)]
pub(super) struct Running {
    pub container: ContainerId,
    pub run: u32,
    pub stdin: Stdin,
    pub activity: Activity,
    /// The background tasks of its that run, by their ids.
    pub tasks: BTreeSet<String>,
    pub reading: Reading,
    /// Its logs, followed as it writes them.
    pub logs: Flow,
    /// Its attach, which its stdin is written through.
    pub attach: Flow,
}

/// Whether a stream of a running container's is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Flow {
    /// Open, as the stream of this number.
    Open(Number),
    /// Ended: it is opened again once the container is found still running.
    Closed,
}

/// Its container's stdin.
#[derive(Bound)]
pub(super) enum Stdin {
    /// Being attached: `lines`, each a whole line of stream-json, wait to be written once it is.
    Attaching { lines: Vec<String> },
    /// Attached: written through `writing`.
    Open(ProtocolRef<Writing>),
}

/// What runs in a session whose container runs.
#[derive(Bound)]
pub(super) enum Activity {
    /// A turn runs, or a line was written and its turn has not ended.
    Working,
    /// No turn runs, but a background task does: it is not idle.
    Waiting,
    /// Nothing runs: once the idle time is up, `sleep`, rest `rest` ends, and it is wound down.
    Idle { sleep: JobId, rest: u64 },
    /// The hub told it it is about to be compacted, and waits for that turn's result until `deadline`.
    Noticed { deadline: JobId, rest: u64 },
    /// The hub asked it to compact, and waits for the result until `deadline`.
    Compacting { deadline: JobId, rest: u64 },
}

impl Session {
    /// A new session named `name`, about to start its first container.
    pub fn new(name: String, opening: Held<Opened>) -> Self {
        Self {
            name,
            conversation: Conversation::Unbegun,
            runs: 1,
            home: Home::Unowned,
            transcript: Transcript::new(),
            life: Life::Starting { run: 1, step: Step::Housing, opening: vec![opening], lines: Vec::new() },
        }
    }

    /// The session as it is listed, numbered `session`.
    pub fn listed(&self, session: u32) -> asks::Session {
        asks::Session { session, name: self.name.clone(), state: self.state(), last: self.transcript.last() }
    }

    /// Where it is, as it is listed.
    pub fn state(&self) -> State {
        match &self.life {
            Life::Starting { .. } => State::Starting,
            Life::Running(running) => match running.activity {
                Activity::Working => State::Working,
                Activity::Waiting => State::Waiting,
                Activity::Idle { .. } => State::Idle,
                Activity::Noticed { .. } => State::Noticed,
                Activity::Compacting { .. } => State::Compacting,
            },
            Life::Stopping { .. } => State::Stopping,
            Life::Stopped => State::Stopped,
            Life::Archived => State::Archived,
        }
    }

    /// Whether its container counts against the most running: it is being started, runs, or is being stopped.
    pub const fn counts(&self) -> bool {
        matches!(self.life, Life::Starting { .. } | Life::Running(_) | Life::Stopping { .. })
    }

    /// Its record, as it stands.
    pub fn record(&self) -> Record {
        let kept = match &self.life {
            Life::Running(running) => Standing::Running { container: running.container.clone(), run: running.run },
            Life::Stopping { container, run, .. } => Standing::Running { container: container.clone(), run: *run },
            Life::Starting { step: Step::Launching { container }, run, .. } => {
                Standing::Running { container: container.clone(), run: *run }
            }
            Life::Starting { .. } | Life::Stopped => Standing::Stopped,
            Life::Archived => Standing::Archived,
        };

        Record {
            name: self.name.clone(),
            conversation: self.conversation.clone(),
            runs: self.runs,
            home: self.home,
            kept,
        }
    }
}
