//! The sessions capability (ADR-0057): Claude sessions the hub runs in containers of their own.
//!
//! Its root is [`Sessions`], which holds the hub's sessions: it runs each awake one in a container made by [`Docker`]
//! only from its settings and the session's number, writes lines to its stdin, reads its output into a numbered
//! transcript kept through `Files`, winds down a session that is idle, so it hibernates, and wakes it when it is spoken
//! to. The host serves it one thing of its own, the read of the credential's file ([`Credential`]), beside the host's randomness and the
//! store's stockroom, which each session's container is readied from: the bearer of its route to the hub's tools,
//! which it holds and checks each request on the sessions' port against as that port's stack's actor layer
//! (ADR-0035), and its drop box (ADR-0057). Behind the `tools` feature,
//! `SessionsTools` is the hub's eight tools for it: `new_session`, `message_session`, `read_session`, `list_sessions`,
//! `interrupt_session`, `hibernate_session`, `wake_session`, and `archive_session`.
//!
//! [`engine_name`] is what the chat calls an engine: a name made from its id alone, `mellow-snipe:4`, so every engine
//! that shows the chat calls one engine alike; [`engine_words`] is its words alone, `mellow-snipe`, as a transcript's
//! tag shows it.
//!
//! [`Docker`] is the hub's client of Docker's Engine API over its Unix socket: it makes, starts, stops, inspects, lists,
//! and removes containers from typed requests, makes named volumes, attaches to a container's stdin and output, reads
//! its logs, and follows the events of the containers that carry a label, each as a typed stream. No request reaches
//! Docker that its asker wrote: a container is made only from a [`ContainerSpec`].

mod docker;
mod names;
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
pub use names::{engine_name, engine_words};
pub use sessions::{
    Act, Acting, Archive, Archived, Asking, AwakeSession, BOOTSTRAP_FAILED, Bearer, CARGO, CLAUDE, Chat, Chatted,
    Compaction, Connection, Conversation, Counted, Credential, Crediting, Ended, Entry, Feed, Grew, HOME, HUB_LABEL,
    Heard, Hibernate, Hibernated, Homing, Index, Interrupt, Interrupted, Joined, Keeping, ListSessions, Loading,
    NewSession, NoCredential, Note, Numbered, Opened, Provision, Provisioned, ReadTranscript, Reconciling, Record,
    Refusal, Resting, SESSION_LABEL, START, Said, Say, SessionRow, Sessions, SessionsListed, SessionsSettings, ShowAsk,
    ShowDeclined, ShowEvents, ShowSessions, Showing, Since, Speaker, Standing, State, TARGET, Transcribed, Transcript,
    Turning, Typing, Unfound, Unlabelled, Wake, WindDown,
};
#[cfg(feature = "tools")]
pub use tools::{Asking as ToolsAsking, Entered, Hibernation, Read, SessionsTools, SessionsToolsConfig};
