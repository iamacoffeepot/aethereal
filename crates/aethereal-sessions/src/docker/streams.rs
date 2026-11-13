//! The streams the Docker client serves (ADR-0017, ADR-0057): an attach to a container's stdin and output, its logs,
//! and the events of the containers that carry a label. Each is paced by its asker's credit: the client reads Docker
//! only while its asker takes what it sends.

use super::{ContainerId, DockerError, Label};
use aethereal_actor::{Bytes, ProtocolRef, Stream};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{HungUp, Writing};

/// Which of a container's output streams a piece of output came on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.std")]
pub enum Std {
    Stdout,
    Stderr,
}

/// A piece of a container's output, on one of its streams, in the order the process wrote it. Pieces of one stream
/// join end to end; a piece is not a line, nor a whole write of the process's.
#[derive(Debug, Bound)]
#[kind(name = "aether.docker.output")]
pub struct Output {
    pub stream: Std,
    pub bytes: Bytes,
}

/// An attach to a container's stdin, stdout, and stderr (ADR-0057): opened with [`Attach`], its first part
/// [`Attached::Stdin`], and every later part the container's output as it comes. Detaching leaves the process
/// running, and a container whose stdin is [`Kept`](super::Stdin::Kept) keeps it open for the next attach.
pub struct Attaching;

impl Stream for Attaching {
    type Open = Attach;
    type Part = Attached;
    type Done = Detached;
}

/// Asked to open an attach to the container.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.attach")]
pub struct Attach {
    pub container: ContainerId,
}

/// One part of an attach.
#[derive(Debug, Bound)]
#[kind(name = "aether.docker.attached")]
pub enum Attached {
    /// Docker took the connection: the container's stdin is written by asking `stdin` to
    /// [`Write`](aethereal_tcp::Write), and [`Shut`](aethereal_tcp::Shut) through it detaches.
    Stdin(ProtocolRef<Writing>),
    /// Output of the container's process.
    Output(Output),
}

/// How an attach ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.detached")]
pub enum Detached {
    /// The connection hung up, as `HungUp` says: [`HungUp::Peer`] when Docker did, as its process ended, and
    /// [`HungUp::Shut`] when its asker detached.
    HungUp(HungUp),
    Failed(DockerError),
}

/// A container's logs (ADR-0057): opened with [`Logs`], each part a piece of what its process wrote, on whichever
/// stream, from `since` on.
pub struct Logging;

impl Stream for Logging {
    type Open = Logs;
    type Part = Output;
    type Done = LogsDone;
}

/// Asked to open a container's logs: what its process wrote from `since_nanos`, nanoseconds since the Unix epoch, to
/// `until`.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.logs")]
pub struct Logs {
    pub container: ContainerId,
    pub since_nanos: u64,
    pub until: Until,
}

/// How far a container's logs are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.until")]
pub enum Until {
    /// To what it has written so far.
    Now,
    /// On, as it writes, until it stops.
    Stopped,
}

/// How a container's logs ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.logs_done")]
pub enum LogsDone {
    /// Every piece asked for was read.
    Read,
    /// Its asker stopped it.
    Stopped,
    Failed(DockerError),
}

/// The events of every container that carries a label (ADR-0057): opened with [`Events`], each part one container
/// starting, dying, or running out of memory, as it happens.
pub struct Watching;

impl Stream for Watching {
    type Open = Events;
    type Part = Event;
    type Done = EventsDone;
}

/// Asked to open the events of every container that carries `label`, from now on.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.events")]
pub struct Events {
    pub label: Label,
}

/// Something that happened to a container.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.event")]
pub enum Event {
    /// It started.
    Started { container: ContainerId },
    /// Its process ended, with `code`.
    Died { container: ContainerId, code: i64 },
    /// The kernel killed a process of its for its memory: it dies next, if that was its main process.
    OutOfMemory { container: ContainerId },
}

/// How the events ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.events_done")]
pub enum EventsDone {
    /// Docker ended them, as a daemon stopping does.
    Closed,
    /// Their asker stopped them.
    Stopped,
    Failed(DockerError),
}
