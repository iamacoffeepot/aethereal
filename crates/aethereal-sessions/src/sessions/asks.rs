//! What the sessions capability is asked, and what it answers (ADR-0057). Each is plain, so the hub's tools take an
//! ask as their input and answer what it answers.

use super::row::SessionRow;
use super::transcript::Numbered;
use crate::DockerError;
use aethereal_actor::Time;
use aethereal_data::Plain;

/// Asked: open a new session, named `name`. Answered once its container runs, or why it does not.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.new_session")]
pub struct NewSession {
    /// What the session is called where it is listed: only shown, and never part of its container.
    #[flag(position)]
    pub name: String,
}

/// Asked: write `text` to the session as a user's message. A hibernating session is woken; one with a turn running
/// takes the line after it, in order.
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

/// What a new session came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.opened")]
pub enum Opened {
    /// It runs.
    Open(SessionRow),
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
    pub sessions: Vec<SessionRow>,
    /// The hub's time as it listed them, the clock each row's time is on.
    pub now: Time,
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
    /// The session has no container, so there is no turn to interrupt.
    NotRunning,
    /// The session's container is starting or stopping: ask again once it is not.
    Busy,
    /// The credential's file could not be read, or does not read as `KEY=VALUE` lines.
    NoCredential,
    /// The host's randomness drew no bearer for the session's route to the hub's tools.
    NoBearer,
    /// The store could not make the session's drop box.
    NoDropBox,
    /// Docker refused, or could not be reached.
    Docker(DockerError),
    /// The session's record, saying it starts, was not written, so nothing of it was made that a hub starting again
    /// would not find.
    Unrecorded,
}

/// An answer that may refuse its ask: what a trusted engine whose chat frame it answers is declined with.
pub(super) trait Refuses {
    /// The refusal it is, when it is one.
    fn refusal(&self) -> Option<&Refusal>;
}

impl Refuses for Opened {
    fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Open(_) => None,
            Self::Refused(why) => Some(why),
        }
    }
}

impl Refuses for Said {
    fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Taken { .. } => None,
            Self::Refused(why) => Some(why),
        }
    }
}

impl Refuses for Interrupted {
    fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Sent { .. } => None,
            Self::Refused(why) => Some(why),
        }
    }
}

impl Refuses for Archived {
    fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Done => None,
            Self::Refused(why) => Some(why),
        }
    }
}
