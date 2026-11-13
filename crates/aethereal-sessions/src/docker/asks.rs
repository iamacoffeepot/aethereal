//! What the Docker client is asked, and what it answers with, a request each (ADR-0057).

use super::{ContainerSpec, Docker};
use aethereal_actor::{Carries, Unanswered};
use aethereal_data::Plain;
use aethereal_http::{Failure, Unreached};

/// A container Docker made, by the id it gave it: 64 lowercase hex digits. A request names a container only by such
/// an id, never by a name or a path, so it names one container Docker made and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Plain)]
#[kind(name = "aether.docker.container_id")]
pub struct ContainerId {
    id: String,
}

/// A label a container carries: a key and its value.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.label")]
pub struct Label {
    pub key: String,
    pub value: String,
}

/// Asked of the client: make a container from `spec`, which is not started.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.create")]
pub struct Create {
    pub spec: ContainerSpec,
}

/// What a create came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.created")]
pub enum Created {
    /// Docker made the container.
    Made(ContainerId),
    Failed(DockerError),
}

/// Asked of the client: start the container.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.start")]
pub struct Start {
    pub container: ContainerId,
}

/// Asked of the client: stop the container, sending its process SIGTERM, and SIGKILL once `grace_secs` have passed.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.stop")]
pub struct Stop {
    pub container: ContainerId,
    pub grace_secs: u32,
}

/// Asked of the client: send the container's process SIGKILL.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.kill")]
pub struct Kill {
    pub container: ContainerId,
}

/// Asked of the client: remove the container, stopping it first if it runs, and the anonymous volumes it made.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.remove")]
pub struct Remove {
    pub container: ContainerId,
}

/// What a start, a stop, a kill, or a remove came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.acted")]
pub enum Acted {
    /// Docker did it.
    Done,
    /// There was nothing to do: the container was started, or stopped, already.
    Unchanged,
    Failed(DockerError),
}

/// Asked of the client: what state the container is in.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.inspect")]
pub struct Inspect {
    pub container: ContainerId,
}

/// What an inspect came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.inspected")]
pub enum Inspected {
    State(ContainerState),
    Failed(DockerError),
}

/// The state a container is in, as its `State` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.container_state")]
pub enum ContainerState {
    /// Made, and never started.
    Created,
    Running,
    Paused,
    Restarting,
    /// Being removed.
    Removing,
    /// Its process ended with `code`, and the kernel killed it for its memory when `oom_killed`.
    Exited {
        code: i64,
        oom_killed: bool,
    },
    /// Docker could not stop or remove it, and gave up on it.
    Dead {
        code: i64,
        oom_killed: bool,
    },
}

/// Asked of the client: every container that carries `label`, running or not.
#[derive(Debug, Plain)]
#[kind(name = "aether.docker.list")]
pub struct List {
    pub label: Label,
}

/// What a list came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.listed")]
pub enum Listed {
    /// The containers, in the order Docker listed them.
    Containers(Vec<Listing>),
    Failed(DockerError),
}

/// One container a list found.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.listing")]
pub struct Listing {
    pub id: ContainerId,
    /// Its name, without the `/` Docker writes before it.
    pub name: String,
    pub phase: Phase,
    /// Every label it carries, by key.
    pub labels: Vec<Label>,
}

/// Where a container a list found is in its life: its state, as a list names it, which carries no exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.phase")]
pub enum Phase {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
}

/// Why the client answered no result.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.error")]
pub enum DockerError {
    /// Docker has no such container (`404`), saying so in `message`.
    NotFound { message: String },
    /// The container's state conflicts with the request (`409`): its name is taken, it is not running, or it is being
    /// removed, as `message` says.
    Conflict { message: String },
    /// Docker answered another error, of `status`.
    Refused { status: u16, message: String },
    /// Docker's socket could not be reached, for the reason `Http` gave: [`Unreached::Missing`] when no socket is at
    /// the path, and [`Unreached::Refused`] when one is and nothing listens on it.
    Unreachable(Unreached),
    /// A reply that does not read as what Docker documents for it.
    Malformed,
    /// The exchange failed before its reply was whole, as `Http`'s client says.
    Broken(Failure),
    /// `Http`'s client answered nothing, as the engine says: it ended, or the request's deadline came first.
    Unanswered(Unanswered),
    /// Docker wrote an error into a stream it had already begun, saying `message`.
    Stream { message: String },
    /// The request could not be written from what it was given, and nothing was sent.
    Unwritable(Unwritable),
}

/// Why a request could not be written.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.unwritable")]
pub enum Unwritable {
    /// An environment variable's name is empty or holds `=`.
    Variable { name: String },
    /// A label's key is empty, or, where a list or the events are filtered by it, holds `=`.
    Label { key: String },
    /// A container's id is not one Docker gave: it would not name one container in a request's path.
    Container,
}

/// Asked whole, it answers once Docker has.
impl Carries<Create> for Docker {
    type Answers = (Created,);
}

impl Carries<Start> for Docker {
    type Answers = (Acted,);
}

impl Carries<Stop> for Docker {
    type Answers = (Acted,);
}

impl Carries<Kill> for Docker {
    type Answers = (Acted,);
}

impl Carries<Remove> for Docker {
    type Answers = (Acted,);
}

impl Carries<Inspect> for Docker {
    type Answers = (Inspected,);
}

impl Carries<List> for Docker {
    type Answers = (Listed,);
}

/// The most hex digits of an id Docker gives.
const ID_DIGITS: usize = 64;

impl ContainerId {
    /// The id `id` writes, when it is one Docker gives: 64 lowercase hex digits.
    #[must_use]
    pub fn read(id: &str) -> Option<Self> {
        let is_id = Self::is_id(id);

        is_id.then(|| Self { id: String::from(id) })
    }

    /// Whether `id` is one Docker gives.
    fn is_id(id: &str) -> bool {
        let is_long = id.len() == ID_DIGITS;
        let is_hex = id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));

        is_long && is_hex
    }

    /// The id as a request's path names it, when it still is one Docker gives: a reference read back from bytes may
    /// hold anything, and is refused.
    pub(super) fn segment(&self) -> Result<&str, Unwritable> {
        let is_id = Self::is_id(&self.id);
        if !is_id {
            return Err(Unwritable::Container);
        }

        Ok(&self.id)
    }

    /// The id, as Docker wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl Label {
    /// The label as a filter names it, `key=value`.
    ///
    /// # Errors
    ///
    /// The key is empty or holds `=`, so the filter would name another label.
    pub(super) fn filter(&self) -> Result<String, Unwritable> {
        let is_key = !self.key.is_empty() && !self.key.contains('=');
        if !is_key {
            return Err(Unwritable::Label { key: self.key.clone() });
        }

        Ok(format!("{}={}", self.key, self.value))
    }
}
