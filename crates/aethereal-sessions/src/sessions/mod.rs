//! The sessions capability's root (ADR-0057): [`Sessions`], which runs each open Claude session in a Docker container
//! of its own, writes lines to its stdin, reads its output into a numbered transcript, stops it when it is idle, and
//! resumes it when it is spoken to.

/// Everything the capability sends in a turn: requests of Docker, its streams and their credit, and writes to stdin;
/// and, for a turn that handles an ask, the answer to it. One list, as each turn may start, stop, or write to any
/// session, its markers always first and in this order, so [`Does`] finds each at its place.
macro_rules! acts {
    ($($answer:ty)?) => {
        (
            super::M0, super::M1, super::M2, super::M3, super::M4, super::M5, super::M6, super::M7, super::M8,
            super::M9, super::M10, super::M11, super::M12, $($answer,)?
        )
    };
}

mod asked;
mod asks;
mod container;
mod disk;
mod heard;
mod idle;
mod lifecycle;
mod loading;
mod output;
mod record;
mod session;
mod settings;
mod streams;
mod transcript;

pub use asks::{
    Archive, Archived, Interrupt, Interrupted, ListSessions, Listed as SessionsListed, NewSession, Opened,
    ReadTranscript, Refusal, Said, Say, Session as ListedSession, State, Transcribed,
};
pub use disk::{
    Fetching, Find, Finding, Found, FoundSession, Keep, Keeping, Kept, Loading, Making, Prepare, Prepared, Preparing,
    SessionsDisk, Storing, Unkept, Write as DiskWrite, Wrote,
};
pub use heard::Heard;
pub use record::{Conversation, Record, Standing};
pub use settings::{CLAUDE, HOME, HUB_LABEL, SESSION_LABEL, SessionsSettings, Unlabelled};
pub use transcript::{Ended, Entry, Note, Numbered, Since, Speaker, Transcript, Turning, WindDown};

use crate::{Attaching, ContainerId, Create, Docker, Inspect, List, Logging, Remove, Start, Stop, Watching};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Asks, At, Beginning, ChildOf, Ending, Fault, Handles, Level, More, Number, Open,
    Part, Path, Streamed, Streams, ToAny, Top, Turn,
};
use aethereal_data::{Bound, Plain, to_bytes};
use aethereal_tcp::{Write, Writing};
use asked::Waiting;
use core::mem;
use core::time::Duration;
use session::Session;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// How many parts of each container's logs and attach are read ahead of what the capability has taken.
const WINDOW: NonZeroU32 = match NonZeroU32::new(4) {
    Some(window) => window,
    None => NonZeroU32::MIN,
};

/// What the capability keeps for a request of Docker about one container of a session: the session, and the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.asking")]
pub struct Asking {
    pub session: u32,
    pub run: u32,
}

/// What the capability keeps for a start, a stop, or a removal of a session's container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.acting")]
pub struct Acting {
    pub session: u32,
    pub run: u32,
    pub act: Act,
}

/// Which of a start, a stop, and a removal it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.act")]
pub enum Act {
    Start,
    Stop,
    Remove,
}

/// What the capability keeps for the list it reconciles its sessions by, as it begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.reconciling")]
pub struct Reconciling;

/// What the capability keeps for a stream of a session's container: the session, and the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.feed")]
pub struct Feed {
    pub session: u32,
    pub run: u32,
}

/// What the capability keeps for a write to a session's stdin: the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.typing")]
pub struct Typing {
    pub session: u32,
}

/// What the capability keeps for a wait on a session's idle time or a step's deadline: the session, and the rest it
/// ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.resting")]
pub struct Resting {
    pub session: u32,
    pub rest: u64,
}

/// Why the capability failed: it could not read its sessions, found no Docker client, or had no room to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.unfound")]
pub enum Unfound {
    /// The host serves no disk, or could not read it.
    Disk,
    /// No Docker client is listed before it.
    Docker,
    /// The engine had no room to hold what it writes.
    NoRoom,
}

/// Each of what the capability sends, as [`acts!`] lists it.
type M0 = Asks<Path<Docker>, Create, Asking>;
type M1 = Asks<Path<Docker>, Start, Acting>;
type M2 = Asks<Path<Docker>, Stop, Acting>;
type M3 = Asks<Path<Docker>, Remove, Acting>;
type M4 = Asks<Path<Docker>, Inspect, Asking>;
type M5 = Asks<Path<Docker>, List, Reconciling>;
type M6 = Asks<Streamed<Attaching>, Open<Attaching>, Number>;
type M7 = ToAny<Streamed<Attaching>, More>;
type M8 = Asks<Streamed<Logging>, Open<Logging>, Number>;
type M9 = ToAny<Streamed<Logging>, More>;
type M10 = Asks<Streamed<Watching>, Open<Watching>, Number>;
type M11 = ToAny<Streamed<Watching>, More>;
type M12 = Asks<Writing, Write, Typing>;

/// The sends of a turn that handles no ask.
type Acts = (M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12);

/// A turn's sends that list everything [`Acts`] does, at the same places: what the capability's helpers act through,
/// whichever answer the turn's ask is answered with.
trait Does:
    Allows<M0, At<0>>
    + Allows<M1, At<1>>
    + Allows<M2, At<2>>
    + Allows<M3, At<3>>
    + Allows<M4, At<4>>
    + Allows<M5, At<5>>
    + Allows<M6, At<6>>
    + Allows<M7, At<7>>
    + Allows<M8, At<8>>
    + Allows<M9, At<9>>
    + Allows<M10, At<10>>
    + Allows<M11, At<11>>
    + Allows<M12, At<12>>
{
}

impl<S> Does for S where
    S: Allows<M0, At<0>>
        + Allows<M1, At<1>>
        + Allows<M2, At<2>>
        + Allows<M3, At<3>>
        + Allows<M4, At<4>>
        + Allows<M5, At<5>>
        + Allows<M6, At<6>>
        + Allows<M7, At<7>>
        + Allows<M8, At<8>>
        + Allows<M9, At<9>>
        + Allows<M10, At<10>>
        + Allows<M11, At<11>>
        + Allows<M12, At<12>>
{
}

/// The sessions capability (ADR-0057): asked at its typed path, `path::<Sessions>(())`, to open a session
/// ([`NewSession`]), write a line to one ([`Say`]), read its transcript ([`ReadTranscript`]), list them
/// ([`ListSessions`]), interrupt one's turn ([`Interrupt`]), or archive one ([`Archive`]).
///
/// Each session runs in a container of its own, made by [`Docker`] only from the settings and the session's number,
/// so nothing a caller asks reaches what a container is made of. At most the settings' number run at once. A session
/// with no turn and no background task for the idle time is told it is about to be compacted, compacted, and stopped,
/// and the next line resumes it in a new container on the same home. Every line its container writes and every line
/// written to it is numbered in its transcript, kept on the host's disk with its record, so a hub that starts again
/// knows its sessions: it follows Docker's events, lists the containers that carry its label, and re-attaches to each
/// that still runs, reading what it missed from its logs.
#[derive(Bound)]
#[kind(name = "aether.sessions")]
pub struct Sessions {
    settings: SessionsSettings,
    phase: Phase,
    held: BTreeMap<u32, Session>,
    /// The containers found carrying the hub's label that it has no record of, by name.
    strays: Vec<String>,
    lane: Lane,
    /// What each session has to write: frames to append to its transcript, and its record, rewritten.
    unwritten: BTreeMap<u32, Vec<u8>>,
    attaching: Streams<Attaching, Feed>,
    logging: Streams<Logging, Feed>,
    watching: Streams<Watching, ()>,
    /// The number of the next rest begun.
    next_rest: u64,
}

/// Where the capability is in beginning.
#[derive(Bound)]
enum Phase {
    /// Its sessions are being read from disk; `waiting` asks are answered once they are, and their containers found.
    Loading { waiting: Vec<Waiting> },
    /// Docker lists the containers carrying its label, to be matched with `recorded`, the containers its records say
    /// may still run.
    Listing { docker: ActorRef<Docker>, recorded: Vec<Recorded>, waiting: Vec<Waiting> },
    /// It runs.
    Ready { docker: ActorRef<Docker> },
}

/// A container a session's record says may still run.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
struct Recorded {
    session: u32,
    container: ContainerId,
    run: u32,
}

/// The write the capability is running on the host's disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Lane {
    Idle,
    Writing,
}

impl ChildOf for Sessions {
    type Parent = Top;
}

impl Actor for Sessions {
    type Config = SessionsSettings;
    type Key = ();
    type Handles = (
        NewSession,
        Say,
        ReadTranscript,
        ListSessions,
        Interrupt,
        Archive,
        Answer<Found, Loading>,
        Answer<Prepared, Preparing>,
        Answer<Wrote, Keeping>,
        Answer<aethereal_actor::Slept, Resting>,
        Answer<crate::Created, Asking>,
        Answer<crate::Acted, Acting>,
        Answer<crate::Inspected, Asking>,
        Answer<crate::Listed, Reconciling>,
        Part<Attaching>,
        Answer<crate::Detached, Number>,
        Part<Logging>,
        Answer<crate::LogsDone, Number>,
        Part<Watching>,
        Answer<crate::EventsDone, Number>,
        Answer<aethereal_tcp::Wrote, Typing>,
    );
    type BeginSends = ();
    type EndSends = ();

    /// Reads its sessions from the host's disk.
    fn begin(settings: SessionsSettings, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.run(&Find, &Loading)?;

        Ok(Self {
            settings,
            phase: Phase::Loading { waiting: Vec::new() },
            held: BTreeMap::new(),
            strays: Vec::new(),
            lane: Lane::Idle,
            unwritten: BTreeMap::new(),
            attaching: Streams::new(WINDOW),
            logging: Streams::new(WINDOW),
            watching: Streams::new(WINDOW),
            next_rest: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Wrote, Keeping>> for Sessions {
    type Sends = Acts;

    /// A write is done: the next is run, of what was changed meanwhile.
    fn handle(
        &mut self,
        (answered, Keeping): (aethereal_actor::Answered<Wrote>, Keeping),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        self.lane = Lane::Idle;
        let is_done = answered == aethereal_actor::Answered::Yes(Wrote::Done);
        if !is_done {
            turn.log(Level::Error, format_args!("a session's transcript or record was not written: {answered:?}"));
        }

        self.flush(turn)
    }
}

impl Sessions {
    /// Adds `entry` to the transcript of `session`, to be written, and answers its number: 0 for no such session.
    fn note(&mut self, session: u32, entry: Entry) -> u64 {
        let Some(held) = self.held.get_mut(&session) else {
            return 0;
        };
        let numbered = held.transcript.add(entry);
        let frames = self.unwritten.entry(session).or_default();
        match numbered.frame() {
            Ok(frame) => frames.extend(frame),
            Err(_) => return numbered.number,
        }

        numbered.number
    }

    /// Marks the record of `session` to be written.
    fn dirty(&mut self, session: u32) {
        self.unwritten.entry(session).or_default();
    }

    /// Runs the write of what has changed, unless one runs.
    fn flush<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let is_writing = self.lane == Lane::Writing;
        let is_clean = self.unwritten.is_empty();
        if is_writing || is_clean {
            return Ok(());
        }
        let mut writes = Vec::new();
        for (session, frames) in mem::take(&mut self.unwritten) {
            let Some(held) = self.held.get(&session) else {
                continue;
            };
            let record = to_bytes(&held.record())?;
            let (Ok(frames), Ok(record)) = (turn.put(frames), turn.put(record)) else {
                return Err(Fault::new(&Unfound::NoRoom)?);
            };
            writes.push(DiskWrite { session, frames, record });
        }
        turn.run(&Keep { writes }, &Keeping)?;
        self.lane = Lane::Writing;

        Ok(())
    }

    /// The Docker client, once the capability has found it.
    const fn docker(&self) -> Option<ActorRef<Docker>> {
        match &self.phase {
            Phase::Loading { .. } => None,
            Phase::Listing { docker, .. } | Phase::Ready { docker } => Some(*docker),
        }
    }

    /// When a request of Docker made now stops being waited for, `extra_secs` past the settings' deadline.
    fn due<S: Does>(&self, turn: &Turn<'_, Self, S>, extra_secs: u32) -> aethereal_actor::Time {
        let secs = u64::from(self.settings.deadline_secs) + u64::from(extra_secs);

        turn.now() + Duration::from_secs(secs)
    }

    /// The next rest's number, taken.
    const fn rest(&mut self) -> u64 {
        let rest = self.next_rest;
        self.next_rest += 1;

        rest
    }
}
