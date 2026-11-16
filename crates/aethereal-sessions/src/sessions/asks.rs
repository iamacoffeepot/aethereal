//! What the sessions capability is asked, and what it answers (ADR-0057). Each is plain, so the hub's tools take an
//! ask as their input and answer what it answers.

use super::Sessions;
use super::transcript::Numbered;
use crate::DockerError;
use aethereal_actor::Carries;
use aethereal_data::Plain;

/// Asked: open a new session, named `name`. Answered once its container runs, or why it does not.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.new_session")]
pub struct NewSession {
    /// What the session is called where it is listed: only shown, and never part of its container.
    #[flag(position)]
    pub name: String,
}

/// Asked: write `text` to the session as a user's message. A session stopped as idle is resumed; one with a turn
/// running takes the line after it, in order.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.say")]
pub struct Say {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
    /// The message.
    #[flag(position)]
    pub text: String,
}

/// Asked: the session's transcript after the entry numbered `after`, at most `most` entries; 0 reads from the start.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.read_transcript")]
pub struct ReadTranscript {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
    /// The number of the last entry already read: 0 for none.
    #[flag]
    pub after: u64,
    /// The most entries answered; 0 for the capability's own most.
    #[flag]
    pub most: u32,
}

/// Asked: every session, and the containers found carrying the hub's label that it has no record of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.list_sessions")]
pub struct ListSessions;

/// Asked: interrupt the session's running turn.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.interrupt")]
pub struct Interrupt {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
}

/// Asked: archive the session: its container is stopped and removed, and its transcript and home are kept.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.archive")]
pub struct Archive {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
}

/// A session as it is listed.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.listed_session")]
pub struct Session {
    /// Its number, which every ask names it by.
    pub session: u32,
    pub name: String,
    pub state: State,
    /// The number of its transcript's last entry.
    pub last: u64,
}

/// Where a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.state")]
pub enum State {
    /// Its container is being made and started.
    Starting,
    /// A turn runs.
    Working,
    /// No turn runs, but a background task of its does.
    Waiting,
    /// Nothing runs in it: it waits for a line, and is stopped once it has waited the idle time.
    Idle,
    /// It was idle, and has been told it is about to be compacted.
    Noticed,
    /// It was idle, and is being compacted before it is stopped.
    Compacting,
    /// Its container is being stopped.
    Stopping,
    /// No container of it runs: the next line resumes it.
    Stopped,
    /// It was archived, and never runs again.
    Archived,
}

/// What a new session came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.opened")]
pub enum Opened {
    /// It runs.
    Open(Session),
    Refused(Refusal),
}

/// What a line came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.said")]
pub enum Said {
    /// It is the transcript's entry of this number, and goes to the session in order.
    Taken {
        number: u64,
    },
    Refused(Refusal),
}

/// A piece of a session's transcript.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.transcript")]
pub enum Transcribed {
    /// The entries asked for, in order, and the number of the transcript's last entry: the next read is after the
    /// last entry answered.
    Entries {
        entries: Vec<Numbered>,
        last: u64,
    },
    Refused(Refusal),
}

/// Every session, and what else carries the hub's label.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.sessions_listed")]
pub struct Listed {
    pub sessions: Vec<Session>,
    /// How many may run at once.
    pub most_running: u32,
    /// The containers found carrying the hub's label that it has no record of, by name: reported, never removed.
    pub strays: Vec<String>,
}

/// What an interrupt came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.interrupted")]
pub enum Interrupted {
    /// It was written to the session, as the transcript's entry of this number.
    Sent {
        number: u64,
    },
    Refused(Refusal),
}

/// What an archive came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.archived")]
pub enum Archived {
    /// The session is archived, and no container of it is left.
    Done,
    Refused(Refusal),
}

/// Why an ask was refused.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.refusal")]
pub enum Refusal {
    /// No session has that number.
    NoSuchSession,
    /// The session is archived.
    Archived,
    /// As many sessions run as may: `most`, these.
    Full { most: u32, running: Vec<u32> },
    /// The session's container does not run, so there is no turn to interrupt.
    NotRunning,
    /// The session is starting or stopping: ask again once it is not.
    Busy,
    /// The host could not make the session's directories or keep its records.
    Disk,
    /// The credential's file could not be read, or does not read as `KEY=VALUE` lines.
    NoCredential,
    /// Docker refused, or could not be reached.
    Docker(DockerError),
}

impl Carries<NewSession> for Sessions {
    type Answers = (Opened,);
}

impl Carries<Say> for Sessions {
    type Answers = (Said,);
}

impl Carries<ReadTranscript> for Sessions {
    type Answers = (Transcribed,);
}

impl Carries<ListSessions> for Sessions {
    type Answers = (Listed,);
}

impl Carries<Interrupt> for Sessions {
    type Answers = (Interrupted,);
}

impl Carries<Archive> for Sessions {
    type Answers = (Archived,);
}
