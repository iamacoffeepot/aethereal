//! One session as the capability holds it: its record, its transcript, and where its container is in its life.

use super::asked::Asker;
use super::asks::{Archived, Opened};
use super::output::Reading;
use super::record::{Conversation, Record, Standing};
use super::row::{Connection, SessionRow, State};
use super::saves::Landing;
use super::transcript::Transcript;
use crate::ContainerId;
use aethereal_actor::{JobId, Number, ProtocolRef};
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
    pub transcript: Transcript,
    pub life: Life,
}

/// Where a session's container is in its life.
#[derive(Bound)]
pub(super) enum Life {
    /// Its container of `run` is being made and started: `step` says how far. `opening` waits to be answered once it
    /// runs, and `lines` wait to be written to it once its stdin is attached.
    Starting { run: u32, step: Step, opening: Vec<Asker<Opened>>, lines: Vec<String> },
    /// Its container runs.
    Running(Box<Running>),
    /// Its container of `run` is being stopped, then removed, and then the session is left as `then` says.
    Stopping { container: ContainerId, run: u32, then: Then },
    /// No container of it exists: it hibernates, and the next line wakes it. `gone` says how its last container ended.
    Hibernating { gone: Gone },
    /// It was archived, and never wakes again.
    Archived,
}

/// How a hibernating session's last container ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Gone {
    /// The hub stopped and removed it, or it never started, or the hub has made none since it began.
    Removed,
    /// It ended without the hub ending it: its process exited, the kernel killed it for its memory, or it was gone
    /// when the hub came back to it.
    Lost,
}

/// How far a container being started is.
#[derive(Bound)]
pub(super) enum Step {
    /// The session's record, saying it starts this run, is written: nothing of the run is made until it lands, so a
    /// hub that starts again finds whatever is.
    Recording,
    /// Docker makes the next of the volumes it mounts, `made` of them made already.
    Housing { made: u32 },
    /// The credential is read.
    Crediting,
    /// Docker makes it.
    Creating,
    /// Docker starts it.
    Launching { container: ContainerId },
}

/// What a session is left as once its container is stopped and removed.
#[derive(Bound)]
pub(super) enum Then {
    /// Hibernating, its container gone as `gone` says: it was idle, or its container ended by itself. `lines` said
    /// meanwhile wake it once its container is removed.
    Rest { lines: Vec<String>, gone: Gone },
    /// Archived, `archiving` answered once it is.
    Archive { archiving: Vec<Asker<Archived>> },
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
    Tasked,
    /// Nothing runs: once the idle time is up, `sleep`, rest `rest` ends, and it is wound down.
    Idle { sleep: JobId, rest: u64 },
    /// The hub told it it is about to be compacted, and waits for that turn's result until `deadline`.
    Noticed { deadline: JobId, rest: u64 },
    /// The hub asked it to compact, and waits for the result until `deadline`.
    Compacting { deadline: JobId, rest: u64 },
}

impl Session {
    /// A new session named `name`, about to start its first container, `opener` answered once it runs.
    pub fn new(name: String, opener: Asker<Opened>) -> Self {
        Self {
            name,
            conversation: Conversation::Unbegun,
            runs: 1,
            transcript: Transcript::new(),
            life: Life::Starting { run: 1, step: Step::Recording, opening: vec![opener], lines: Vec::new() },
        }
    }

    /// The session as it is listed, numbered `session`, with `asking` of its asks waiting for the owner.
    pub fn row(&self, session: u32, asking: u32) -> SessionRow {
        let (state, container) = self.states(asking);
        let grew = self.transcript.grew();

        SessionRow { session, name: self.name.clone(), state, container, grew, asking }
    }

    /// Its conversation's state and its container's, with `asking` of its asks waiting for the owner. Lines that wait
    /// for its container are a turn to come, so it is working; a container the hub stops for an idle session leaves it
    /// idle until Docker has removed it, as nothing runs in it and a line said meanwhile wakes it again.
    fn states(&self, asking: u32) -> (State, Connection) {
        match &self.life {
            Life::Starting { lines, .. } => (waits(lines), Connection::Starting),
            Life::Running(_) if asking > 0 => (State::Waiting, Connection::Attached),
            Life::Running(running) => (running.activity.state(), Connection::Attached),
            Life::Stopping { then: Then::Rest { gone: Gone::Lost, .. }, .. }
            | Life::Hibernating { gone: Gone::Lost } => (State::Hibernating, Connection::Lost),
            Life::Stopping { then: Then::Rest { gone: Gone::Removed, lines }, .. } => {
                (waits(lines), Connection::Attached)
            }
            Life::Stopping { then: Then::Archive { .. }, .. } => (State::Archived, Connection::Attached),
            Life::Hibernating { gone: Gone::Removed } => (State::Hibernating, Connection::None),
            Life::Archived => (State::Archived, Connection::None),
        }
    }

    /// Whether its container counts against the most running: it is being started, runs, or is being stopped.
    pub const fn counts(&self) -> bool {
        matches!(self.life, Life::Starting { .. } | Life::Running(_) | Life::Stopping { .. })
    }

    /// Its record, as it stands: from the start of a run until its container is removed, it says a container may
    /// exist, and names it.
    pub fn record(&self) -> Record {
        let kept = match &self.life {
            Life::Running(running) => Standing::Awake { container: running.container.clone(), run: running.run },
            Life::Stopping { container, run, .. } => Standing::Awake { container: container.clone(), run: *run },
            Life::Starting { step: Step::Launching { container }, run, .. } => {
                Standing::Awake { container: container.clone(), run: *run }
            }
            Life::Starting { run, .. } => Standing::Starting { run: *run },
            Life::Hibernating { .. } => Standing::Hibernating,
            Life::Archived => Standing::Archived,
        };

        Record { name: self.name.clone(), conversation: self.conversation.clone(), runs: self.runs, kept }
    }

    /// What its record's write, as it stands, lets go ahead once it lands, as the session numbered `session`: the
    /// start of its run, while that waits on its record.
    pub const fn landing(&self, session: u32) -> Landing {
        match &self.life {
            Life::Starting { run, step: Step::Recording, .. } => Landing::Starting { session, run: *run },
            Life::Starting { .. }
            | Life::Running(_)
            | Life::Stopping { .. }
            | Life::Hibernating { .. }
            | Life::Archived => Landing::Nothing,
        }
    }

    /// Whether a container started now carries on Claude's conversation rather than beginning one.
    pub const fn resumes(&self) -> bool {
        matches!(self.conversation, Conversation::Begun(_))
    }
}

impl Activity {
    /// The conversation's state while its container runs and no ask waits: the notice's turn, as it is wound down, and
    /// a background task are each work.
    const fn state(&self) -> State {
        match self {
            Self::Working | Self::Tasked | Self::Noticed { .. } => State::Working,
            Self::Idle { .. } => State::Idle,
            Self::Compacting { .. } => State::Compacting,
        }
    }
}

/// The state of a session no turn runs in, with `lines` waiting for its container: working, as each is a turn to come,
/// or idle when none waits.
const fn waits(lines: &[String]) -> State {
    if lines.is_empty() {
        State::Idle
    } else {
        State::Working
    }
}
