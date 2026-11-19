//! What the sessions capability is asked, and what it answers (ADR-0057). Each is plain, so the hub's tools take an
//! ask as their input and answer what it answers.

use super::row::SessionRow;
use super::transcript::Numbered;
use crate::DockerError;
use aethereal_actor::Time;
use aethereal_data::Plain;
use core::fmt;

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

/// Asked: wind the session down to hibernate now, as an idle one is: told it is about to be compacted and hibernated,
/// compacted, and its container stopped and removed. Answered as it stands once its wind-down has begun, not once it
/// hibernates.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.hibernate")]
pub struct Hibernate {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
}

/// Asked: wake the hibernating session in a new container on its home, which resumes its conversation. Answered once
/// it runs, as a new session is, or why not.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.wake")]
pub struct Wake {
    /// The session's number.
    #[flag(position)]
    pub session: u32,
}

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

/// What a new session, or a wake, came to.
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

/// What a hibernate came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.hibernated")]
pub enum Hibernated {
    /// It is being wound down: told, compacted, and its container stopped. The list shows it hibernating once it is.
    WindingDown,
    /// It hibernates already.
    Hibernating,
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
    /// As many sessions are awake as may: `most`, these, so the owner chooses one to hibernate.
    Full { most: u32, awake: Vec<AwakeSession> },
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
    /// A turn, or a background task, runs in the session: a hibernate would cut it short, so it is interrupted first.
    Working,
    /// A turn of the session waits on the owner's answer to an ask: a hibernate would leave it unanswered, so it is
    /// answered first.
    Waiting,
    /// The session is being compacted, as its owner or Claude Code asked: a hibernate would cut the compaction short, so
    /// it is asked again once the compaction ends.
    Compacting,
}

/// An awake session, as a refusal past the most awake names it: by its number, and its name as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.awake_session")]
pub struct AwakeSession {
    pub session: u32,
    pub name: String,
}

/// What a refusal says, for its asker to read: what the hub's tools answer, and what an engine prints of a frame it
/// declined.
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchSession => f.write_str("no session has that number"),
            Self::Archived => f.write_str("the session is archived"),
            Self::Full { most, awake } => {
                write!(f, "at most {most} sessions are awake at once, and these are:")?;
                for (place, AwakeSession { session, name }) in awake.iter().enumerate() {
                    let comma = if place == 0 {
                        ""
                    } else {
                        ","
                    };
                    write!(f, "{comma} {session} `{name}`")?;
                }
                f.write_str("; hibernate one first")
            }
            Self::NotRunning => f.write_str("the session has no container, so no turn runs"),
            Self::Busy => f.write_str("the session's container is starting or stopping: ask again once it is not"),
            Self::NoCredential => f.write_str("the hub could not read the credential's file as KEY=VALUE lines"),
            Self::NoBearer => f.write_str("the hub's randomness drew no bearer for the session's route"),
            Self::NoDropBox => f.write_str("the store could not make the session's drop box"),
            Self::Docker(error) => write!(f, "Docker: {error:?}"),
            Self::Unrecorded => f.write_str("the hub could not write the session's record, so it made nothing"),
            Self::Working => f.write_str("a turn or a background task runs in the session: interrupt it first"),
            Self::Waiting => {
                f.write_str("a turn of the session waits on the owner's answer to an ask: answer it first")
            }
            Self::Compacting => f.write_str("the session is being compacted: ask again once the compaction ends"),
        }
    }
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

impl Refuses for Hibernated {
    fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::WindingDown | Self::Hibernating => None,
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
