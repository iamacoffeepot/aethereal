//! What the reconcile reads of a transcript, kept as each entry is taken: a hub that starts again rebuilds it from the
//! transcript on disk, entry by entry, so it holds nothing the transcript does not.

use super::transcript::{Entry, Event, LineId, Role, TaskId, What};
use aethereal_actor::Time;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A transcript, as the reconcile reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Ledger {
    /// How many entries it has taken: the number of the latest, as the first is 1.
    count: u64,
    /// Every line said, in order.
    said: Vec<Said>,
    /// Each line Claude has read, and the run it first read it in.
    receipted: BTreeMap<LineId, u32>,
    /// What each container's output says, by its run and role.
    containers: BTreeMap<(u32, Role), Copied>,
    since: Since,
    interrupts: Vec<Interrupted>,
    /// The latest marker of each run, by its entry's number.
    markers: BTreeMap<u32, u64>,
    /// The containers the hub found gone.
    gone: BTreeSet<(u32, Role)>,
}

/// A line said: its entry's number, its id, and when it was said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Said {
    pub number: u64,
    pub line: LineId,
    pub at: Time,
}

/// An entry's number and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mark {
    pub number: u64,
    pub at: Time,
}

/// The hub's interrupt of a turn: its entry, its run, and the entry of the turn it interrupts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Interrupted {
    pub mark: Mark,
    pub run: u32,
    pub turn: u64,
}

/// Whether a turn began since the last compaction finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Since {
    /// None did: there is nothing new to compact.
    #[default]
    Compacted,
    /// One did.
    Turned,
}

/// What one container's output, copied into the transcript, says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Copied {
    /// How many lines of its log the transcript holds.
    pub lines: u64,
    /// The time of the latest of them.
    pub latest: Time,
    pub read: Read,
    pub resume: Resume,
    pub activity: Activity,
}

/// Whether Claude read any line in a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Read {
    Nothing,
    Something,
}

/// Whether a container's `--resume` found no conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resume {
    Unrefused,
    Failed,
}

/// What runs in a Claude Code container, as its output says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Activity {
    turn: Turn,
    /// The receipts of lines whose turns have not begun, in order.
    waiting: VecDeque<Mark>,
    /// The background tasks that run, by id, each with the entry that began it.
    tasks: BTreeMap<TaskId, Mark>,
    compaction: Compaction,
}

/// Whether a turn runs, and the entry that began it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    Idle,
    Running(Mark),
}

/// Whether a compaction runs, and the entry that began it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compaction {
    Idle,
    Running(Mark),
}

/// The activity of a container with no output.
static QUIET: Activity = Activity::new();

impl Activity {
    const fn new() -> Self {
        Self { turn: Turn::Idle, waiting: VecDeque::new(), tasks: BTreeMap::new(), compaction: Compaction::Idle }
    }

    /// Whether anything runs or waits to: a turn, a line read whose turn has not begun, a task, or a compaction.
    #[must_use]
    pub fn busy(&self) -> bool {
        let compacting = matches!(self.compaction, Compaction::Running(_));

        self.unanswered() || compacting
    }

    /// Whether something Claude read or began is unfinished: a turn, a line read whose turn has not begun, or a task. A
    /// container that ends so cut it short.
    #[must_use]
    pub fn unanswered(&self) -> bool {
        let turning = matches!(self.turn, Turn::Running(_));

        turning || !self.waiting.is_empty() || !self.tasks.is_empty()
    }

    /// Whether a turn runs.
    #[must_use]
    pub const fn turning(&self) -> bool {
        matches!(self.turn, Turn::Running(_))
    }

    /// The entry the current work began at, which its deadline runs from: the turn's, else the compaction's, else the
    /// first line read and waiting for its turn, else the first task's. None when nothing runs.
    #[must_use]
    pub fn anchor(&self) -> Option<Mark> {
        match (self.turn, self.compaction) {
            (Turn::Running(mark), _) | (Turn::Idle, Compaction::Running(mark)) => Some(mark),
            (Turn::Idle, Compaction::Idle) => {
                let task = || self.tasks.values().min_by_key(|mark| mark.number).copied();
                self.waiting.front().copied().or_else(task)
            }
        }
    }

    fn take(&mut self, mark: Mark, event: &Event, line: Fresh) {
        match event {
            Event::Receipt(LineId::Compact { .. }) | Event::Compacted => self.compaction = Compaction::Idle,
            Event::Receipt(_) => {
                if matches!(line, Fresh::Yes) {
                    self.waiting.push_back(mark);
                }
            }
            Event::Init => {
                self.waiting.pop_front();
                self.turn = Turn::Running(mark);
            }
            Event::Result => self.turn = Turn::Idle,
            Event::TaskStarted(task) => {
                self.tasks.insert(task.clone(), mark);
            }
            Event::TaskEnded(task) => {
                self.tasks.remove(task);
            }
            Event::Compacting => self.compaction = Compaction::Running(mark),
            Event::ResumeFailed => {}
        }
    }
}

/// Whether a receipt is the first of its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Fresh {
    Yes,
    No,
}

impl Copied {
    const fn new(at: Time) -> Self {
        Self { lines: 0, latest: at, read: Read::Nothing, resume: Resume::Unrefused, activity: Activity::new() }
    }
}

impl Ledger {
    /// The ledger of an empty transcript.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the next entry of the transcript.
    pub fn take(&mut self, entry: &Entry) {
        self.count += 1;
        let mark = Mark { number: self.count, at: entry.at };
        match &entry.what {
            What::Said(line) => self.said.push(Said { number: mark.number, line: *line, at: entry.at }),
            What::Output { run, role, event } => self.output(mark, *run, *role, event),
            What::Attached { run } => {
                self.markers.insert(*run, mark.number);
            }
            What::Interrupted { run, turn } => self.interrupts.push(Interrupted { mark, run: *run, turn: *turn }),
            What::Gone { run, role } => {
                self.gone.insert((*run, *role));
            }
        }
    }

    fn output(&mut self, mark: Mark, run: u32, role: Role, event: &Event) {
        let fresh = match event {
            Event::Receipt(line) if !self.receipted.contains_key(line) => {
                self.receipted.insert(*line, run);
                Fresh::Yes
            }
            Event::Receipt(_)
            | Event::Init
            | Event::Result
            | Event::TaskStarted(_)
            | Event::TaskEnded(_)
            | Event::Compacting
            | Event::Compacted
            | Event::ResumeFailed => Fresh::No,
        };
        if matches!(event, Event::Init) {
            self.since = Since::Turned;
        }
        if matches!(event, Event::Compacted) {
            self.since = Since::Compacted;
        }
        let copied = self.containers.entry((run, role)).or_insert_with(|| Copied::new(mark.at));
        copied.lines += 1;
        copied.latest = copied.latest.max(mark.at);
        if matches!(event, Event::Receipt(_)) {
            copied.read = Read::Something;
        }
        if matches!(event, Event::ResumeFailed) {
            copied.resume = Resume::Failed;
        }
        copied.activity.take(mark, event, fresh);
    }

    /// How many entries it holds: the number of the latest.
    #[must_use]
    pub const fn entries(&self) -> u64 {
        self.count
    }

    /// Every line said, in order.
    #[must_use]
    pub fn said(&self) -> &[Said] {
        &self.said
    }

    /// Whether Claude has read `line`.
    #[must_use]
    pub fn receipted(&self, line: LineId) -> bool {
        self.receipted.contains_key(&line)
    }

    /// Whether `line` was said.
    #[must_use]
    pub fn has_said(&self, line: LineId) -> bool {
        self.said.iter().any(|said| said.line == line)
    }

    /// What `run`'s container `role` wrote, when it wrote anything.
    #[must_use]
    pub fn container(&self, run: u32, role: Role) -> Option<&Copied> {
        self.containers.get(&(run, role))
    }

    /// How many lines of `run`'s container `role`'s log the transcript holds.
    #[must_use]
    pub fn copied(&self, run: u32, role: Role) -> u64 {
        self.container(run, role).map_or(0, |copied| copied.lines)
    }

    /// What runs in `run`'s Claude Code container, as its output says.
    #[must_use]
    pub fn activity(&self, run: u32) -> &Activity {
        self.container(run, Role::Claude).map_or(&QUIET, |copied| &copied.activity)
    }

    /// Whether a turn began since the last compaction finished.
    #[must_use]
    pub const fn turned(&self) -> bool {
        matches!(self.since, Since::Turned)
    }

    /// The hub's interrupt of the turn in `run` that the entry numbered `turn` began.
    #[must_use]
    pub fn interrupt(&self, run: u32, turn: u64) -> Option<&Interrupted> {
        self.interrupts.iter().find(|interrupted| interrupted.run == run && interrupted.turn == turn)
    }

    /// The number of `run`'s latest marker.
    #[must_use]
    pub fn marker(&self, run: u32) -> Option<u64> {
        self.markers.get(&run).copied()
    }

    /// Whether the hub found `run`'s container `role` gone.
    #[must_use]
    pub fn gone(&self, run: u32, role: Role) -> bool {
        self.gone.contains(&(run, role))
    }

    /// The number of the latest line said at or before `at`: 0 when there is none.
    #[must_use]
    pub fn said_through(&self, at: Time) -> u64 {
        self.said.iter().filter(|said| said.at <= at).map(|said| said.number).max().unwrap_or(0)
    }

    /// Whether an owner's line after entry `held_through` has not been read: it wakes a hibernating session.
    #[must_use]
    pub fn wakes(&self, held_through: u64) -> bool {
        self.said.iter().any(|said| {
            let owner = matches!(said.line, LineId::Owner(_));

            owner && said.number > held_through && !self.receipted(said.line)
        })
    }
}
