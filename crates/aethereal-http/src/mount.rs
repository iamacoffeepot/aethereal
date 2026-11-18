//! Mounts (ADR-0035): a path prefix whose requests a server hands to a handler of its own with the body streamed as it
//! arrives, paced by the handler's credit, in place of gathered whole.

use crate::{Header, Method};
use aethereal_actor::{Bytes, Carries, Protocol, ProtocolRef, Stream, Streamed};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{HungUp, SocketAddress};

/// A path prefix a server streams the requests under: each request whose path starts with `prefix` is asked of
/// `handler` as a [`StreamedRequest`], and its body, decoded, may run to `most_body_bytes`, which stands in for the
/// server's `most_body_bytes`. A request under no mount is asked of the server's handler whole.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.mount")]
pub struct Mount {
    pub prefix: String,
    pub most_body_bytes: u64,
    pub handler: ProtocolRef<StreamHandling>,
}

/// The mount a request to `path` goes to: the first of `mounts` whose prefix starts it, none when the request goes to
/// the server's handler whole.
pub fn mounted<'m>(mounts: &'m [Mount], path: &str) -> Option<&'m Mount> {
    mounts.iter().find(|mount| path.starts_with(mount.prefix.as_str()))
}

/// What a mount's handler takes: each [`StreamedRequest`] under its prefix, asked of it and answered with a
/// [`Response`](crate::Response) (ADR-0035). A handler is named in a [`Mount`] by a reference to it of this protocol.
pub struct StreamHandling;

impl Protocol for StreamHandling {
    type Kinds = (StreamedRequest,);
}

impl Carries<StreamedRequest> for StreamHandling {
    type Answers = (crate::Response,);
}

/// A request an exchange parsed the head of, every limit on the head checked, whose body is read to its handler as it
/// arrives through an [`InboundBody`] stream the handler opens on `body` with [`read_body`](Self::read_body).
#[derive(Debug, Bound)]
#[kind(name = "aether.http.streamed_request")]
pub struct StreamedRequest {
    pub method: Method,
    /// The target's path, as sent.
    pub path: String,
    /// What follows the target's `?`, empty when there is none.
    pub query: String,
    /// In the order sent, duplicates kept, each name lowercased.
    pub headers: Vec<Header>,
    /// How long its body is, as its head says.
    pub length: BodyLength,
    /// The exchange that reads the body.
    pub body: ProtocolRef<Streamed<InboundBody>>,
    /// Which of the exchange's requests it is, which the stream that reads its body names.
    pub(crate) request: u64,
    /// Where the request came from.
    pub peer: SocketAddress,
    /// The port it came in on: the server's.
    pub port: u16,
}

impl StreamedRequest {
    /// What an [`InboundBody`] stream on `body` opens with to read this request's body.
    #[must_use]
    pub const fn read_body(&self) -> ReadBody {
        ReadBody { request: self.request }
    }
}

/// How long a streamed request's body is, as its head says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.body_length")]
pub enum BodyLength {
    /// Its `Content-Length`: 0 for a request with no body.
    Known(u64),
    /// In chunks: its length is known once the last has come.
    Chunked,
}

/// A stream of one request's body as it arrives (ADR-0017, ADR-0035), served by the stack or the exchange that reads its
/// connection: opened with [`ReadBody`], each part the bytes of the body one read brought, decoded and passed by the
/// layers, and ended with how the body was read. The reader's credit paces it: the connection is read only while the
/// reader has credit, so a reader that falls behind holds the client back, and the body is never held whole.
pub struct InboundBody;

impl Stream for InboundBody {
    type Open = ReadBody;
    type Part = BodyBytes;
    type Done = BodyRead;
}

/// What an [`InboundBody`] stream opens with: the request whose body it reads, from a stack's
/// [`BodyHandle::read`](crate::stack::BodyHandle::read) or [`StreamedRequest::read_body`].
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

    /// Whether it reads the body of the request numbered `request`.
    pub(crate) const fn reads(self, request: u64) -> bool {
        self.request == request
    }
}

/// Bytes of a streamed request's body, decoded, in the order they came.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.body_bytes")]
pub struct BodyBytes {
    pub bytes: Bytes,
}

/// How a streamed request's body was read: the answer that ends its [`InboundBody`] stream, told once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.body_read")]
pub enum BodyRead {
    /// It ended where its framing said: every byte was sent. The connection takes the next request once the handler
    /// has answered.
    Whole,
    /// It ran past what is taken of it: past its mount's `most_body_bytes`, which an exchange answers `413`, or a
    /// stack's layer refused it as too large. The rest is not read, and the connection closes after the response.
    TooLarge,
    /// Its chunked framing broke, answered `400`, or `413` for a chunk's line past `most_head_bytes`, and the
    /// connection closes once that is sent; or a stack's layer refused it as malformed.
    Malformed,
    /// It made no progress for `stall_within_millis`: no bytes came, or its reader opened no stream or granted no
    /// credit. The connection closes, and owes no response.
    Stalled,
    /// The connection ended before the body did, as `HungUp` says, or its server was shut. The connection has closed.
    HungUp(HungUp),
    /// It names no body waiting to be read: another stream or gather reads it, it has been read, or the request it
    /// names is not its connection's request out. Nothing more of the body this stream would read is sent.
    Busy,
    /// The handler answered before the body ended, or the reader stopped the stream: the rest of it is not read, and
    /// the connection closes once the response is sent.
    Cut,
    /// An actor layer of a stack did not answer a part of it, and the stack answers the request itself.
    Unpassed,
}
