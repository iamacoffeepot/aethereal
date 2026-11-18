//! A request's body streamed to its handler as it arrives (ADR-0035): the stream a handler opens on its request's
//! [`BodyHandle`](crate::stack::BodyHandle), and how long the body is, as its head says.

use aethereal_actor::{Bytes, Stream};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::HungUp;

/// How long a request's body is, as its head says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.body_length")]
pub enum BodyLength {
    /// Its `Content-Length`: 0 for a request with no body.
    Known(u64),
    /// In chunks: its length is known once the last has come.
    Chunked,
}

/// A stream of one request's body as it arrives (ADR-0017, ADR-0035), served by the stack that reads its connection:
/// opened with [`ReadBody`], each part the bytes of the body one read brought, decoded and passed by the layers, and
/// ended with how the body was read. The reader's credit paces it: the connection is read only while the reader has
/// credit, so a reader that falls behind holds the client back, and the body is never held whole.
pub struct InboundBody;

impl Stream for InboundBody {
    type Open = ReadBody;
    type Part = BodyBytes;
    type Done = BodyRead;
}

/// What an [`InboundBody`] stream opens with: the request whose body it reads, from its
/// [`BodyHandle::read`](crate::stack::BodyHandle::read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.read_body")]
pub struct ReadBody {
    request: u64,
}

impl ReadBody {
    /// The open of a stream that reads the body of the request numbered `request`.
    pub(crate) const fn new(request: u64) -> Self {
        Self { request }
    }

    /// Which request it reads the body of.
    pub(crate) const fn request(self) -> u64 {
        self.request
    }
}

/// Bytes of a request's body, decoded and passed by the layers, in the order they came.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.body_bytes")]
pub struct BodyBytes {
    pub bytes: Bytes,
}

/// How a request's body was read: the answer that ends its [`InboundBody`] stream, told once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.body_read")]
pub enum BodyRead {
    /// It ended where its framing said: every byte was sent. The connection takes the next request once the handler
    /// has answered.
    Whole,
    /// A layer refused it as too large. The rest is not read, and the connection closes after the response.
    TooLarge,
    /// Its chunked framing broke, answered `400`, or `413` for a chunk's line past `most_head_bytes`, and the
    /// connection closes once that is sent; or a layer refused it as malformed.
    Malformed,
    /// It made no progress for `stall_within_millis`: no bytes came, or its reader opened no stream or granted no
    /// credit. The connection closes, and owes no response.
    Stalled,
    /// The connection ended before the body did, as `HungUp` says, or its stack was shut. The connection has closed.
    HungUp(HungUp),
    /// It names no body waiting to be read: another stream or gather reads it, it has been read, or the request it
    /// names is not its connection's request out. Nothing more of the body this stream would read is sent.
    Busy,
    /// The handler answered before the body ended, or the reader stopped the stream: the rest of it is not read, and
    /// the connection closes once the response is sent.
    Cut,
    /// An actor layer did not answer a part of it, and the stack answers the request itself.
    Unpassed,
}
