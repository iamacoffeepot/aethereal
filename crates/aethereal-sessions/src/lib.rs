//! The sessions capability (ADR-0057): Claude sessions the hub runs in containers of their own.
//!
//! Its root is [`Sessions`], which holds the hub's sessions: it runs each in a container made by [`Docker`] only from
//! its settings and the session's number, writes lines to its stdin, reads its output into a numbered transcript kept
//! on the host's disk through [`SessionsDisk`], winds down and stops a session that is idle, and resumes it when it is
//! spoken to. Behind the `tools` feature, `SessionsTools` is the hub's six tools for it: `session_new`,
//! `session_say`, `session_read`, `session_list`, `session_interrupt`, and `session_archive`.
//!
//! [`Docker`] is the hub's client of Docker's Engine API over its Unix socket: it makes, starts, stops, inspects,
//! lists, and removes containers from typed requests, attaches to a container's stdin and output, reads its logs, and
//! follows the events of the containers that carry a label, each as a typed stream. No request reaches Docker that its
//! asker wrote: a container is made only from a [`ContainerSpec`].

mod docker;
mod sessions;
#[cfg(feature = "tools")]
mod tools;

pub use docker::{
    API_VERSION, Access, Acted, Attach, Attached, Attaching, Bind, Calling, ContainerId, ContainerSpec, ContainerState,
    Create, Created, Detached, Dialing, Docker, DockerConfig, DockerError, DockerSettings, Event, Events, EventsDone,
    Fetches, Inspect, Inspected, Kill, Label, List, Listed, Listing, Logging, Logs, LogsDone, Network, Output, Phase,
    Remove, SocketUnaddressable, Start, Std, Stdin, Stop, Until, Unwritable, Variable, Watching,
};
pub use sessions::{
    Act, Acting, Archive, Archived, Asking, CLAUDE, Conversation, DiskWrite, Ended, Entry, Feed, Fetching, Find,
    Finding, Found, FoundSession, HOME, HUB_LABEL, Heard, Interrupt, Interrupted, Keep, Keeping, Kept, ListSessions,
    ListedSession, Loading, Making, NewSession, Note, Numbered, Opened, Prepare, Prepared, Preparing, ReadTranscript,
    Reconciling, Record, Refusal, Resting, SESSION_LABEL, Said, Say, Sessions, SessionsDisk, SessionsListed,
    SessionsSettings, Since, Speaker, Standing, State, Storing, Transcribed, Transcript, Turning, Typing, Unfound,
    Unkept, Unlabelled, WindDown, Wrote,
};
#[cfg(feature = "tools")]
pub use tools::{Asking as ToolsAsking, Entered, Read, SessionsTools, SessionsToolsConfig};
