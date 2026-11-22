//! The simulated world: the record and the transcript on disk, the hub's memory, Docker's containers with their logs
//! and stdin, the fake Claude Code in each, the home volume's conversation, what the owner has said, and the clock.

use super::bounds::Bounds;
use crate::reconcile::{
    Attachment, Begin, Desired, Entry, Ledger, LineId, Output, Record, Refusal, Role, Run, What, When,
};
use aethereal_actor::Time;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

/// Everything there is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct World {
    pub now: Time,
    pub sessions: Vec<Session>,
    pub spent: Spent,
    pub image: Image,
    pub repository: Repository,
}

/// What the explorer's bounds have spent so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(super) struct Spent {
    pub hub_crashes: u8,
    /// Faults of every kind, together.
    pub faults: u8,
    /// Which kinds of fault happened, one bit each.
    pub kinds: u32,
    pub tasks: u8,
}

/// Whether the session image is there to make containers from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Image {
    Present,
    Missing,
}

/// Whether the hub serves its repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Repository {
    Served,
    Unserved,
}

/// One session: its files, the hub's memory of it, its containers, its home, and what the explorer knows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Session {
    pub record: Record,
    pub transcript: Vec<Entry>,
    /// The transcript's ledger, kept as each entry is appended.
    pub ledger: Ledger,
    pub memory: Memory,
    pub containers: BTreeMap<(u32, Role), Container>,
    /// Claude's conversation on the home volume: the lines it saved, in order, and those whose turns finished.
    pub home: Home,
    pub truth: Truth,
}

/// The hub's memory of a session, lost when it stops.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Memory {
    pub attachment: Attachment,
    pub refused: Refused,
}

/// Docker's refusal of the hub's last ask of a container, as the hub holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Refused {
    Nothing,
    Refused { run: u32, role: Role, refusal: Refusal },
}

/// Claude's conversation files.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(super) struct Home {
    pub saved: Vec<LineId>,
    pub finished: BTreeSet<LineId>,
}

/// What the explorer knows that no part of the system does, for its invariants.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Truth {
    /// How many lines the owner has said.
    pub said: u32,
    /// Every line Claude ever saved, in order, whatever became of the home since.
    pub saved: Vec<LineId>,
    /// How many turns each line began.
    pub turns: BTreeMap<LineId, u8>,
    /// The runs whose Claude Code container ended with a line it read unanswered.
    pub cut: BTreeSet<u32>,
    /// Lines whose receipt was in output removed before the hub read it: the hub cannot know Claude read them.
    pub unknowable: BTreeSet<LineId>,
    /// Whether the owner asked since the session last went to sleep: a wake, or a line said.
    pub armed: bool,
    /// Which asks the owner has made, one bit each, for the asks bounded to one.
    pub asked: u8,
}

/// A container, as Docker holds it, with the process in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Container {
    pub state: Docked,
    pub log: Vec<Output>,
    pub begin: Begin,
    pub pipe: Vec<Piped>,
    pub process: Process,
    pub asked_stop: bool,
    pub ended: Ended,
}

/// A container's state in Docker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Docked {
    Created,
    Running { started: Time },
    Exited { code: i32, oom: bool, finished: Time },
}

/// What was written to a container's stdin and not yet read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Piped {
    Line(LineId),
    Interrupt,
    /// The start of a line whose writer died.
    Fragment,
}

/// The process in a container.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Process {
    /// Not started.
    Made,
    /// Claude Code has started and not yet read its flags against its home.
    Booting,
    /// Claude Code reads its stdin.
    Live(Claude),
    /// The bootstrap script runs.
    Bootstrapping,
    Dead,
}

/// A live Claude Code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(super) struct Claude {
    /// The ids it read in this process.
    pub seen: BTreeSet<LineId>,
    /// Lines read whose turns have not begun.
    pub queue: VecDeque<LineId>,
    pub doing: Doing,
    pub tasks: Tasks,
}

/// What a live Claude Code is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(super) enum Doing {
    #[default]
    Idle,
    Turn {
        line: LineId,
        since: Time,
    },
    Compacting {
        line: LineId,
        since: Time,
    },
}

/// Its background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(super) enum Tasks {
    #[default]
    None,
    One {
        since: Time,
    },
}

/// How a container ended, and where the record stood for it as it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Ended {
    Not,
    Ended { cause: Cause, state: Run },
}

/// Why a process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Cause {
    /// The hub asked Docker to stop it.
    Asked,
    Crash,
    OutOfMemory,
    /// Docker restarted.
    Daemon,
    /// It read a line cut short, then more.
    Torn,
    /// `--session-id` of a conversation that exists.
    InUse,
    /// `--resume` of one that does not.
    Unresumable,
    /// The bootstrap's own exit.
    Bootstrap,
}

/// Whether a time is compared only with the clock, or with other stored times too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    /// Compared only as `now >= at + limit`.
    Anchor,
    /// Also compared with other times of this class: when a line was said, and when a container exited.
    Ranked,
}

/// The start of every clock in a canonical world.
const BASE: u64 = 1 << 20;

impl World {
    pub fn new(bounds: &Bounds) -> Self {
        let session = Session {
            record: Record::new(),
            transcript: Vec::new(),
            ledger: Ledger::new(),
            memory: Memory::new(),
            containers: BTreeMap::new(),
            home: Home::default(),
            truth: Truth {
                said: 0,
                saved: Vec::new(),
                turns: BTreeMap::new(),
                cut: BTreeSet::new(),
                unknowable: BTreeSet::new(),
                armed: true,
                asked: 0,
            },
        };

        let mut sessions = vec![session; bounds.sessions];
        for hibernating in sessions.iter_mut().skip(bounds.cap) {
            hibernating.record.desired = Desired::Hibernating(When::AfterTurn);
        }

        Self {
            now: Time::from_nanos(BASE),
            sessions,
            spent: Spent::default(),
            image: Image::Present,
            repository: Repository::Served,
        }
    }

    /// Every stored time, each with its class.
    fn visit(&mut self, visit: &mut impl FnMut(&mut Time, Class)) {
        for session in &mut self.sessions {
            for entry in &mut session.transcript {
                let class = if matches!(entry.what, What::Said(_)) {
                    Class::Ranked
                } else {
                    Class::Anchor
                };
                visit(&mut entry.at, class);
            }
            for container in session.containers.values_mut() {
                match &mut container.state {
                    Docked::Created => {}
                    Docked::Running { started } => visit(started, Class::Anchor),
                    Docked::Exited { finished, .. } => visit(finished, Class::Ranked),
                }
                for output in &mut container.log {
                    visit(&mut output.at, Class::Anchor);
                }
                if let Process::Live(claude) = &mut container.process {
                    if let Doing::Turn { since, .. } | Doing::Compacting { since, .. } = &mut claude.doing {
                        visit(since, Class::Anchor);
                    }
                    if let Tasks::One { since } = &mut claude.tasks {
                        visit(since, Class::Anchor);
                    }
                }
            }
        }
    }

    /// Makes the world canonical: every time shifted so the clock reads `BASE`, a time older than every deadline
    /// (`longest`) that is compared only with the clock made the oldest that still counts as old, and the old times
    /// compared with each other ranked, ties kept. Two worlds that differ only so behave alike under every comparison
    /// the reconcile and the explorer make.
    pub fn canonical(&mut self, longest: u64) {
        let now = self.now.nanos();
        let mut old = Vec::new();
        self.visit(&mut |at, class| {
            let aged = now - at.nanos() >= longest;
            if aged && class == Class::Ranked {
                old.push(at.nanos());
            }
        });
        old.sort_unstable_by(|a, b| b.cmp(a));
        old.dedup();
        self.visit(&mut |at, class| {
            let age = now - at.nanos();
            let mapped = match class {
                _ if age < longest => BASE - age,
                Class::Anchor => BASE - longest,
                Class::Ranked => {
                    let rank = old.iter().position(|value| *value == at.nanos()).unwrap_or(0) as u64;
                    BASE - longest - 1 - rank
                }
            };
            *at = Time::from_nanos(mapped);
        });
        self.now = Time::from_nanos(BASE);
        for session in &mut self.sessions {
            let mut ledger = Ledger::new();
            for entry in &session.transcript {
                ledger.take(entry);
            }
            session.ledger = ledger;
        }
    }

    /// The fingerprint the explorer knows a world by: two hashes of it, taken in one pass.
    pub fn fingerprint(&self) -> u128 {
        let mut twin = Twin { first: DefaultHasher::new(), second: DefaultHasher::new() };
        0x9e37_79b9_7f4a_7c15_u64.hash(&mut twin.second);
        self.hash(&mut twin);

        (u128::from(twin.first.finish()) << 64) | u128::from(twin.second.finish())
    }
}

/// Two hashers fed the same bytes, from different starts.
struct Twin {
    first: DefaultHasher,
    second: DefaultHasher,
}

impl Hasher for Twin {
    fn finish(&self) -> u64 {
        self.first.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.first.write(bytes);
        self.second.write(bytes);
    }
}

/// A session's state, without its ledger, which its transcript decides.
impl Hash for Session {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.record.hash(state);
        self.transcript.hash(state);
        self.memory.hash(state);
        self.containers.hash(state);
        self.home.hash(state);
        self.truth.hash(state);
    }
}

impl Memory {
    pub const fn new() -> Self {
        Self { attachment: Attachment::Detached, refused: Refused::Nothing }
    }
}

impl Session {
    pub const fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Appends `entry` to the transcript, and its ledger takes it.
    pub fn append(&mut self, entry: Entry) {
        self.ledger.take(&entry);
        self.transcript.push(entry);
    }

    /// The container the record's phase names, when Docker holds it.
    pub fn named(&self) -> Option<(u32, Role)> {
        let (role, _) = self.record.phase.container()?;

        Some((self.record.run, role))
    }
}

impl Container {
    pub const fn made(begin: Begin) -> Self {
        Self {
            state: Docked::Created,
            log: Vec::new(),
            begin,
            pipe: Vec::new(),
            process: Process::Made,
            asked_stop: false,
            ended: Ended::Not,
        }
    }

    pub const fn running(&self) -> bool {
        matches!(self.state, Docked::Running { .. })
    }
}
