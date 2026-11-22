//! One session as the capability holds it: its record, its transcript, and where its container is in its life.

use super::asked::Asker;
use super::asks::{Archived, Opened};
use super::output::Reading;
use super::record::{Conversation, Record, Standing};
use super::row::{Connection, SessionRow, State};
use super::saves::Landing;
use super::transcript::{Compaction, Transcript};
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
    /// Its run `run` starts: its bootstrap container is made and runs, and once it has exited 0, its Claude Code
    /// container is made and started, as `step` says. `opening` waits to be answered once Claude Code's container is
    /// attached, and `lines`, each the number of its entry in the transcript, a line said or run, wait to be written to
    /// it then; should the start fail first, the session holds them for its next run.
    Starting { run: u32, step: Step, opening: Vec<Asker<Opened>>, lines: Vec<u64> },
    /// Its Claude Code container runs.
    Running(Box<Running>),
    /// Its container of `run` is being stopped, then removed, and then the session is left as `then` says.
    Stopping { container: ContainerId, run: u32, then: Then },
    /// No container of it exists: it hibernates, and the next line, or a wake, wakes it. `gone` says how its last
    /// container ended.
    Hibernating { gone: Gone },
    /// It was archived, and never wakes again.
    Archived,
}

/// How a hibernating session's last container ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Gone {
    /// The hub stopped and removed it, or the hub has made none since it began.
    Removed,
    /// It ended without the hub ending it: its process exited, the kernel killed it for its memory, or it was gone
    /// when the hub came back to it.
    Lost,
    /// Its start failed before Claude Code ran, as `why` says: a container of it was never made or started, the hub's
    /// repository was not served, its bootstrap container did not exit 0, or the hub stopped before the start finished
    /// and a hub that started again found no container of it. None of the lines that waited for it were written:
    /// `unread`, each the number of its entry in the transcript, in order, are written first to its next container,
    /// before any line that wakes it, and noted nowhere again. Empty when there were none.
    Unstarted { unread: Vec<u64>, why: String },
}

/// Which of a run's two containers a step of its start makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Stage {
    /// The bootstrap container, whose command is the start script alone: it readies the session's work tree and
    /// exits, and its exit code is its outcome.
    Bootstrap,
    /// Claude Code's container, made only once the bootstrap container exited 0 and was removed.
    Claude,
}

/// How far a run being started is.
#[derive(Bound)]
pub(super) enum Step {
    /// The session's record, saying it starts this run, is written before `stage`'s container is made: nothing of it
    /// is made until the record lands, so a hub that starts again finds whatever is.
    Recording { stage: Stage },
    /// Docker makes the next of the volumes the run's containers mount, `made` of them made already.
    Housing { made: u32 },
    /// The credential is read, and a bearer drawn, for `stage`'s container.
    Crediting { stage: Stage },
    /// Docker makes `stage`'s container.
    Creating { stage: Stage },
    /// Docker starts `stage`'s container.
    Launching { stage: Stage, container: ContainerId },
    /// The bootstrap container runs: its logs are followed from its start into the transcript, as `reading` reads
    /// them, for people to read. Once they end, as they do only once it has stopped, Docker's state of it says how it
    /// ended, and the start is judged by that alone.
    Bootstrapping { container: ContainerId, reading: Reading, logs: Flow },
    /// The bootstrap container exited 0, and Docker removes it: its bearer is let go of once it is, and Claude Code's
    /// container is made.
    Clearing { container: ContainerId },
}

/// What a session is left as once its container is stopped and removed.
#[derive(Bound)]
pub(super) enum Then {
    /// Hibernating, its container gone as `gone` says: it was idle, asked to hibernate, or its container ended by
    /// itself. A wake or a line meanwhile wakes it again once its container is removed: `opening` waits to be answered
    /// once its next container runs, and `lines`, each the number of its entry in the transcript, to be written to it.
    Hibernate { opening: Vec<Asker<Opened>>, lines: Vec<u64>, gone: Gone },
    /// Archived, `archiving` answered once it is.
    Archive { archiving: Vec<Asker<Archived>> },
}

/// A session whose Claude Code container runs.
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
            life: Life::Starting {
                run: 1,
                step: Step::Recording { stage: Stage::Bootstrap },
                opening: vec![opener],
                lines: Vec::new(),
            },
        }
    }

    /// The session as it is listed, numbered `session`, with `asking` of its asks waiting for the owner.
    pub fn row(&self, session: u32, asking: u32) -> SessionRow {
        let (state, container) = self.states(asking);
        let grew = self.transcript.grew();

        SessionRow { session, name: self.name.clone(), state, container, grew, asking }
    }

    /// Its conversation's state as it is listed, with `asking` of its asks waiting for the owner.
    pub fn state(&self, asking: u32) -> State {
        self.states(asking).0
    }

    /// Its conversation's state and its container's, with `asking` of its asks waiting for the owner. Lines that wait
    /// for its container are a turn to come, so it is working; a compaction Claude Code runs is compacting, whoever
    /// asked for it; a container the hub stops for a session to hibernate leaves it idle until Docker has removed it,
    /// as nothing runs in it and a line said meanwhile wakes it again.
    fn states(&self, asking: u32) -> (State, Connection) {
        match &self.life {
            Life::Starting { lines, .. } => (waits(lines), Connection::Starting),
            Life::Running(_) if asking > 0 => (State::Waiting, Connection::Attached),
            Life::Running(running) => {
                let state = match self.transcript.compaction() {
                    Compaction::Running => State::Compacting,
                    Compaction::Idle => running.activity.state(),
                };
                (state, Connection::Attached)
            }
            Life::Stopping { then: Then::Hibernate { gone: Gone::Lost, .. }, .. }
            | Life::Hibernating { gone: Gone::Lost } => (State::Hibernating, Connection::Lost),
            Life::Stopping {
                then: Then::Hibernate { gone: Gone::Removed | Gone::Unstarted { .. }, lines, .. },
                ..
            } => (waits(lines), Connection::Attached),
            Life::Stopping { then: Then::Archive { .. }, .. } => (State::Archived, Connection::Attached),
            Life::Hibernating { gone: Gone::Removed } => (State::Hibernating, Connection::None),
            Life::Hibernating { gone: Gone::Unstarted { unread, why } } => {
                let holding = u32::try_from(unread.len()).unwrap_or(u32::MAX);
                (State::Hibernating, Connection::Unstarted { why: why.clone(), holding })
            }
            Life::Archived => (State::Archived, Connection::None),
        }
    }

    /// Whether it is awake, and so counts against the most awake at once: its container is being started, runs, or is
    /// being stopped and removed.
    pub const fn counts(&self) -> bool {
        matches!(self.life, Life::Starting { .. } | Life::Running(_) | Life::Stopping { .. })
    }

    /// Its record, as it stands: from the start of a run until its last container is removed, it says a container may
    /// exist, and names it, its bootstrap container apart from its Claude Code container; once it is removed, whether
    /// its last container was lost, so a hub that starts again shows it lost until it wakes, or why its last start
    /// failed and the lines that start never wrote, for its next.
    pub fn record(&self) -> Record {
        let kept = match &self.life {
            Life::Running(running) => Standing::Awake { container: running.container.clone(), run: running.run },
            Life::Stopping { container, run, .. }
            | Life::Starting { step: Step::Launching { stage: Stage::Claude, container }, run, .. } => {
                Standing::Awake { container: container.clone(), run: *run }
            }
            Life::Starting {
                step:
                    Step::Launching { stage: Stage::Bootstrap, container }
                    | Step::Bootstrapping { container, .. }
                    | Step::Clearing { container },
                run,
                ..
            } => Standing::Bootstrapping { container: container.clone(), run: *run },
            Life::Starting { run, .. } => Standing::Starting { run: *run },
            Life::Hibernating { gone: Gone::Removed } => Standing::Hibernating,
            Life::Hibernating { gone: Gone::Lost } => Standing::Lost,
            Life::Hibernating { gone: Gone::Unstarted { unread, why } } => {
                Standing::Unstarted { unread: unread.clone(), why: why.clone() }
            }
            Life::Archived => Standing::Archived,
        };

        Record { name: self.name.clone(), conversation: self.conversation.clone(), runs: self.runs, kept }
    }

    /// What its record's write, as it stands, lets go ahead once it lands, as the session numbered `session`: the
    /// start of its run, while that waits on its record.
    pub const fn landing(&self, session: u32) -> Landing {
        match &self.life {
            Life::Starting { run, step: Step::Recording { .. }, .. } => Landing::Starting { session, run: *run },
            Life::Starting { .. }
            | Life::Running(_)
            | Life::Stopping { .. }
            | Life::Hibernating { .. }
            | Life::Archived => Landing::Nothing,
        }
    }

    /// Whether its last container was lost and nothing has asked it to wake since: it hibernates lost, or its lost
    /// container is being removed with no wake or line waiting on it. The next to wake it is preceded by the hub's
    /// line.
    pub fn lost_unwoken(&self) -> bool {
        match &self.life {
            Life::Hibernating { gone: Gone::Lost } => true,
            Life::Stopping { then: Then::Hibernate { gone: Gone::Lost, opening, lines }, .. } => {
                opening.is_empty() && lines.is_empty()
            }
            Life::Starting { .. }
            | Life::Running(_)
            | Life::Stopping { .. }
            | Life::Hibernating { gone: Gone::Removed | Gone::Unstarted { .. } }
            | Life::Archived => false,
        }
    }

    /// Whether a container started now carries on Claude's conversation rather than beginning one.
    pub const fn resumes(&self) -> bool {
        matches!(self.conversation, Conversation::Begun(_))
    }
}

impl Activity {
    /// Whether it is the hub's own wind-down: its notice's turn, or the compaction it asked for.
    pub const fn winds_down(&self) -> bool {
        matches!(self, Self::Noticed { .. } | Self::Compacting { .. })
    }

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
const fn waits(lines: &[u64]) -> State {
    if lines.is_empty() {
        State::Idle
    } else {
        State::Working
    }
}
