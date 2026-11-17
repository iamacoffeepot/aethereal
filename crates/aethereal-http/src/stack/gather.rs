//! A request's body asked for whole (ADR-0035): the protocol a handler asks its stack through, and its answer.

use aethereal_actor::{Bytes, Carries, Protocol};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::HungUp;

/// One request's body, asked for whole: what a [`BodyHandle`](super::BodyHandle)'s `gatherer` takes.
pub struct Gathering;

impl Protocol for Gathering {
    type Kinds = (Gather,);
}

impl Carries<Gather> for Gathering {
    type Answers = (Gathered,);
}

/// Asks a stack for one request's body in one answer, refused past `most_bytes`: made by
/// [`BodyHandle::whole`](super::BodyHandle::whole).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.gather")]
pub struct Gather {
    pub most_bytes: u32,
    /// Which of the stack's requests it reads the body of.
    request: u64,
}

impl Gather {
    /// The gather of the body of the stack's request numbered `request`, refused past `most_bytes`.
    pub(super) const fn new(most_bytes: u32, request: u64) -> Self {
        Self { most_bytes, request }
    }

    /// Which of the stack's requests it reads the body of.
    pub(super) const fn request(&self) -> u64 {
        self.request
    }
}

/// What a gather came to.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.gathered")]
pub enum Gathered {
    /// The body, decoded and passed by the layers, whole: no bytes for a request with no body.
    Whole(Bytes),
    /// It runs past `most_bytes`: by its `Content-Length` before a byte is read, or once a chunked body has. The rest
    /// is not read, and the connection closes after the response.
    TooLarge,
    /// Its chunked framing broke, and the stack answers `400` itself, or a layer refused it as malformed.
    Malformed,
    /// It has not moved for `stall_within_millis`, and the connection has closed with no answer.
    Stalled,
    /// The connection hung up before it ended, as `HungUp` says, or the stack was shut.
    HungUp(HungUp),
    /// It names no body waiting to be read: another gather reads it, it has been read, or its request is not its
    /// connection's request out.
    Busy,
    /// An actor layer did not answer a part of it, and the stack answers the request itself.
    Unpassed,
}
