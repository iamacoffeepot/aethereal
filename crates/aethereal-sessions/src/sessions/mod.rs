//! The sessions capability's root (ADR-0057): [`Sessions`], which runs each open Claude session in a Docker container
//! of its own, writes lines to its stdin, reads its output into a numbered transcript, stops it when it is idle, and
//! resumes it when it is spoken to.

mod asked;
mod asks;
mod bearers;
mod chat;
mod container;
mod credential;
mod heard;
mod idle;
mod layer;
mod lifecycle;
mod loading;
mod output;
mod record;
mod saves;
mod session;
mod settings;
mod streams;
mod transcript;
mod volumes;

pub use asks::{
    Archive, Archived, Interrupt, Interrupted, ListSessions, Listed as SessionsListed, NewSession, Opened,
    ReadTranscript, Refusal, Said, Say, Session as ListedSession, State, Transcribed,
};
pub use bearers::Bearer;
pub use chat::{Chat, Chatted, Joined, ShowAsk, ShowEvents, ShowTabs, Showing, Tab};
pub use credential::{Credential, Crediting, NoCredential, Provision, Provisioned};
pub use heard::Heard;
pub use record::{Conversation, Record, Standing};
pub use saves::{Index, Keeping, Loading};
pub use settings::{CARGO, CLAUDE, HOME, HUB_LABEL, SESSION_LABEL, SessionsSettings, TARGET, Unlabelled};
pub use transcript::{Ended, Entry, Note, Numbered, Since, Speaker, Transcript, Turning, WindDown};
pub use volumes::Homing;

use crate::{
    Acted, Attaching, ContainerId, Created, Detached, Docker, EventsDone, Inspected, Logging, LogsDone, VolumeCreated,
    Watching,
};
use aethereal_actor::{ActorRef, Answered, Beginning, Fault, Later, Number, Part, Slept, Speaks, Streams, Turn, actor};
use aethereal_data::{Bound, Plain};
use aethereal_files::{FileRead, FileWritten, Files, Namespace, ReadFile};
use aethereal_http::stack::{FromStack, Inbound, Inward, Outbound, Outward};
use aethereal_tcp::Wrote;
use aethereal_tools::Caller;
use asked::Waiting;
use bearers::Bearers;
use chat::Chats;
use core::time::Duration;
use loading::Reading;
use saves::{Indexed, Lane};
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
    /// `Files` could not read the sessions' files, or answered nothing, as when it is not listed before them.
    Files,
    /// No Docker client is listed before it.
    Docker,
    /// The engine had no room to hold what it writes.
    NoRoom,
}

/// The sessions capability (ADR-0057): asked at its typed path, `path::<Sessions>(())`, to open a session
/// ([`NewSession`]), write a line to one ([`Say`]), read its transcript ([`ReadTranscript`]), list them
/// ([`ListSessions`]), interrupt one's turn ([`Interrupt`]), or archive one ([`Archive`]).
///
/// Each session runs in a container of its own, made by [`Docker`] only from the settings and the session's number,
/// so nothing a caller asks reaches what a container is made of. At most the settings' number run at once. A session
/// with no turn and no background task for the idle time is told it is about to be compacted, compacted, and stopped,
/// and the next line resumes it in a new container on the same home, a named volume of its own. Every line its
/// container writes and every line written to it is numbered in its transcript, kept with its record in `Files`'
/// `Save` namespace, which it asks as any actor does and needs listed before it, so a hub that starts again knows its
/// sessions: it follows Docker's events, lists the containers that carry its label, and re-attaches to each that still
/// runs, reading what it missed from its logs.
///
/// It is the actor layer of the stack that serves the sessions' port (ADR-0035): each container is given a bearer it
/// mints and holds, and a request on that port bearing one it holds passes on as that session's
/// ([`Inbound`](aethereal_http::stack::Inbound)); any other is answered `401`. It holds only each session's latest
/// container's bearer, and lets go of it as the container is removed.
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
    indexed: Indexed,
    attaching: Streams<Attaching, Feed>,
    logging: Streams<Logging, Feed>,
    watching: Streams<Watching, ()>,
    /// The number of the next rest begun.
    next_rest: u64,
    /// The trusted engines the chat is shown to, and what waits for their owner's answer (ADR-0057).
    chats: Chats,
    /// The bearer of each running session's container, which the container stack's requests are checked against.
    bearers: Bearers,
}

/// Where the capability is in beginning. In each phase before it runs, `waiting` asks are answered once it does.
#[derive(Bound)]
enum Phase {
    /// The index of its sessions is read through `Files`.
    Indexing { waiting: Vec<Waiting> },
    /// Each session the index lists is read, its record and its transcript, and `reading` holds what has come.
    Reading { reading: Reading, waiting: Vec<Waiting> },
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

#[actor(depends(Docker, Files))]
impl Sessions {
    /// Reads the index of its sessions through `Files`.
    fn begin(settings: SessionsSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let index = ReadFile { namespace: Namespace::Save, path: saves::INDEX };
        turn.to(aethereal_actor::path::<Files>(())).ask(&index, &Loading::Index)?;

        Ok(Self {
            settings,
            phase: Phase::Indexing { waiting: Vec::new() },
            held: BTreeMap::new(),
            strays: Vec::new(),
            lane: Lane::Idle,
            unwritten: BTreeMap::new(),
            indexed: Indexed::Written,
            attaching: Streams::new(WINDOW),
            logging: Streams::new(WINDOW),
            watching: Streams::new(WINDOW),
            next_rest: 0,
            chats: Chats::new(),
            bearers: Bearers::default(),
        })
    }

    /// A new session, numbered after every other, its first container started: answered once it runs.
    #[request]
    fn new_session(&mut self, new_session: NewSession, turn: &mut Turn<'_, Self>) -> Result<Later<Opened>, Fault> {
        self.handle_new_session(new_session, turn)
    }

    /// A line for a session: written to it in order, a turn running or not; a stopped session is resumed first.
    #[request]
    fn say(&mut self, say: Say, turn: &mut Turn<'_, Self>) -> Result<Later<Said>, Fault> {
        self.handle_say(say, turn)
    }

    #[request]
    fn read_transcript(
        &mut self,
        read_transcript: ReadTranscript,
        turn: &mut Turn<'_, Self>,
    ) -> Result<Later<Transcribed>, Fault> {
        self.handle_read_transcript(read_transcript, turn)
    }

    #[request]
    fn list_sessions(&mut self, list_sessions: ListSessions) -> Result<SessionsListed, Fault> {
        self.handle_list_sessions(list_sessions)
    }

    /// The session's running turn interrupted, through the control request Claude Code reads on its stdin.
    #[request]
    fn interrupt(&mut self, interrupt: Interrupt, turn: &mut Turn<'_, Self>) -> Result<Later<Interrupted>, Fault> {
        self.handle_interrupt(interrupt, turn)
    }

    /// The session archived: its container stopped and removed, its transcript and home kept. Answered once no
    /// container of it is left.
    #[request]
    fn archive(&mut self, archive: Archive, turn: &mut Turn<'_, Self>) -> Result<Later<Archived>, Fault> {
        self.handle_archive(archive, turn)
    }

    /// A file of the sessions read: the index asks for each session's record and transcript, and once every one is
    /// answered, the sessions are held as stopped until the containers carrying the hub's label are listed.
    #[answer]
    fn file_read(&mut self, answered: (Answered<FileRead>, Loading), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_file_read(answered, turn)
    }

    /// A write is done: the next is sent, and once none waits, those of what changed meanwhile.
    #[answer]
    fn file_written(
        &mut self,
        answered: (Answered<FileWritten>, Keeping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.handle_file_written(answered, turn)
    }

    /// The credential is read, a bearer drawn, and the drop box made: the bearer is held as the session's, and Docker
    /// asked to make the session's container, which is given both.
    #[answer]
    fn provisioned(
        &mut self,
        answered: (Answered<Provisioned>, Crediting),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.handle_provisioned(answered, turn)
    }

    /// A volume of the session's is made: the next is, or once each is, the credential is read.
    #[answer]
    fn volume_created(
        &mut self,
        answered: (Answered<VolumeCreated>, Homing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.handle_volume_created(answered, turn)
    }

    /// A session's idle time, or a step's deadline, is up: unless it was active since, it is wound down a step.
    #[answer]
    fn slept(&mut self, answered: (Answered<Slept>, Resting), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_slept(answered, turn)
    }

    /// Docker made the container: it is recorded, and started. One made for a start given up on is removed.
    #[answer]
    fn created(&mut self, answered: (Answered<Created>, Asking), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_created(answered, turn)
    }

    /// A start, a stop, or a removal Docker did, or why not.
    #[answer]
    fn acted(&mut self, answered: (Answered<Acted>, Acting), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_acted(answered, turn)
    }

    /// The state of a container whose logs or attach ended: one still running is followed again, and one that is not
    /// has ended.
    #[answer]
    fn inspected(&mut self, answered: (Answered<Inspected>, Asking), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_inspected(answered, turn)
    }

    /// The containers carrying the hub's label: each a record says may run is re-attached when it runs, and noted
    /// lost when it does not; each with no record is reported.
    #[answer]
    fn listed(
        &mut self,
        answered: (Answered<crate::Listed>, Reconciling),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.handle_listed(answered, turn)
    }

    /// The container's stdin, attached: each line waiting is written. Its output is read from its logs instead, so
    /// what the attach carries is taken and let go.
    #[tell]
    fn part_attaching(&mut self, part: Part<Attaching>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_part_attaching(part, turn)
    }

    /// A container's attach ended: its state is asked for.
    #[answer]
    fn detached(&mut self, answered: (Answered<Detached>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_detached(answered, turn)
    }

    /// A piece of a container's output: each line it ends goes into the session's transcript, and says what it says
    /// of the session's turns.
    #[tell]
    fn part_logging(&mut self, part: Part<Logging>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_part_logging(part, turn)
    }

    /// A container's logs ended, as they do once it stops: its state is asked for.
    #[answer]
    fn logs_done(&mut self, answered: (Answered<LogsDone>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_logs_done(answered, turn)
    }

    /// An event of a container carrying the hub's label: a session's that died has ended.
    #[tell]
    fn part_watching(&mut self, part: Part<Watching>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_part_watching(part, turn)
    }

    /// Docker's events ended: a container that dies is then found as its logs end.
    #[answer]
    fn events_done(
        &mut self,
        answered: (Answered<EventsDone>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.handle_events_done(answered, turn)
    }

    /// A write to a session's stdin: one that did not go is noted in its transcript.
    #[answer]
    fn wrote(&mut self, answered: (Answered<Wrote>, Typing), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_wrote(answered, turn)
    }

    /// A trusted engine linked: its sender is watched, and shown every session and each ask that waits at once. A
    /// sender that takes nothing an engine is shown is refused by its guard; one that names an engine another actor
    /// joined as, and one joined as another engine already, are each refused here, and nothing joined changes.
    #[tell]
    fn joined(&mut self, joined: Joined, turn: &mut Turn<'_, Self, Speaks<Showing>>) -> Result<(), Fault> {
        self.handle_joined(joined, turn)
    }

    /// What a trusted engine's owner did: taken only from the actor that joined as that engine, and done once the
    /// capability has found its sessions.
    #[tell]
    fn chatted(&mut self, chatted: Chatted, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.handle_chatted(chatted, turn)
    }

    /// An engine that joined has ended: its tabs end with it. A turn running carries on, and an ask waits on.
    #[ended]
    fn ended(&mut self, ended: aethereal_actor::Ended) -> Result<(), Fault> {
        self.handle_ended(ended)
    }

    /// A request bearing a bearer it minted and holds, as a `Bearer` token or a `Basic` password, is passed on as that
    /// session's; any other, bearing none, a bearer it does not hold, or credentials of another scheme, is answered
    /// `401` with a `Bearer` challenge, and goes no further.
    #[request]
    fn inbound(&mut self, inbound: Inbound<()>, turn: &mut Turn<'_, Self, FromStack>) -> Result<Inward<Caller>, Fault> {
        self.handle_inbound(inbound, turn)
    }

    /// Nothing to do on the way out: the head passes on unchanged.
    #[request]
    fn outbound(&mut self, outbound: Outbound, turn: &mut Turn<'_, Self, FromStack>) -> Result<Outward, Fault> {
        self.handle_outbound(outbound, turn)
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

    /// The Docker client, once the capability has found it.
    const fn docker(&self) -> Option<ActorRef<Docker>> {
        match &self.phase {
            Phase::Indexing { .. } | Phase::Reading { .. } => None,
            Phase::Listing { docker, .. } | Phase::Ready { docker } => Some(*docker),
        }
    }

    /// When a request of Docker made now stops being waited for, `extra_secs` past the settings' deadline.
    fn due(&self, turn: &Turn<'_, Self>, extra_secs: u32) -> aethereal_actor::Time {
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
