//! The sessions capability (ADR-0057): Claude sessions the hub runs in containers of their own.
//!
//! Its root is [`Sessions`], which holds the hub's sessions: it runs each in a container made by [`Docker`] only from
//! its settings and the session's number, writes lines to its stdin, reads its output into a numbered transcript kept
//! through `Files`, winds down and stops a session that is idle, and resumes it when it is spoken to. The host serves it
//! one thing of its own, the read of the credential's file ([`Credential`]), beside the host's randomness and the
//! store's stockroom, which each session's container is readied from: the bearer of its route to the hub's tools,
//! which `Mcp` is told of, and its drop box (ADR-0057). Behind the `tools` feature,
//! `SessionsTools` is the hub's six tools for it: `session_new`, `session_say`, `session_read`, `session_list`,
//! `session_interrupt`, and `session_archive`.
//!
//! [`Docker`] is the hub's client of Docker's Engine API over its Unix socket: it makes, starts, stops, inspects, lists,
//! and removes containers from typed requests, makes named volumes, attaches to a container's stdin and output, reads
//! its logs, and follows the events of the containers that carry a label, each as a typed stream. No request reaches
//! Docker that its asker wrote: a container is made only from a [`ContainerSpec`].

mod docker;
mod sessions;
#[cfg(feature = "tools")]
mod tools;

pub use docker::{
    API_VERSION, Access, Acted, Attach, Attached, Attaching, Bind, Calling, ContainerId, ContainerSpec, ContainerState,
    Create, CreateVolume, Created, Detached, Dialing, Docker, DockerConfig, DockerError, DockerSettings, Event, Events,
    EventsDone, Fetches, Inspect, Inspected, Kill, Label, List, Listed, Listing, Logging, Logs, LogsDone, Network,
    Output, Phase, Remove, SocketUnaddressable, Start, Std, Stdin, Stop, Until, Unwritable, Variable, Volume,
    VolumeCreated, VolumeName, Watching,
};
pub use sessions::{
    Act, Acting, Archive, Archived, Asking, CARGO, CLAUDE, Chat, Chatted, Conversation, Credential, Crediting, Ended,
    Entry, Feed, HOME, HUB_LABEL, Heard, Homing, Index, Interrupt, Interrupted, Joined, Keeping, ListSessions,
    ListedSession, Loading, NewSession, NoCredential, Note, Numbered, Opened, Provision, Provisioned, ReadTranscript,
    Reconciling, Record, Refusal, Resting, SESSION_LABEL, Said, Say, Sessions, SessionsListed, SessionsSettings,
    ShowAsk, ShowEvents, ShowTabs, Showing, Since, Speaker, Standing, State, TARGET, Tab, Transcribed, Transcript,
    Turning, Typing, Unfound, Unlabelled, WindDown,
};
#[cfg(feature = "tools")]
pub use tools::{Asking as ToolsAsking, Entered, Read, SessionsTools, SessionsToolsConfig};
