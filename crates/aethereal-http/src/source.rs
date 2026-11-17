//! A streamed response's source (ADR-0035): an actor the handler names in its [`Response`](crate::Response), which
//! the exchange opens a [`ResponseSource`] stream on and writes to the connection as it comes, paced by what the
//! connection has sent.

use aethereal_actor::{Bytes, Stream};
use aethereal_data::{Bound, Plain};

/// An exchange's stream of one response's body from its source (ADR-0017): opened with [`SendBody`], each part bytes of
/// the body in order, and ended with how the source ended it. The exchange grants a part back only once the
/// connection has sent one, so a client that reads slowly holds the source back, and the body is never held whole.
pub struct ResponseSource;

impl Stream for ResponseSource {
    type Open = SendBody;
    type Part = ResponseBytes;
    type Done = SourceEnd;
}

/// What a [`ResponseSource`] stream opens with: the number the handler gave the body in its
/// [`ResponseBody::Streamed`](crate::ResponseBody::Streamed), so one source can send several bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.send_body")]
pub struct SendBody {
    pub body: u64,
}

/// Bytes of a streamed response's body, in order. A part of no bytes writes nothing.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.response_bytes")]
pub struct ResponseBytes {
    pub bytes: Bytes,
}

/// How a source ended a response's body: the answer that ends its [`ResponseSource`] stream, told once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.source_end")]
pub enum SourceEnd {
    /// Every byte was sent: the exchange ends the body as its framing says, and takes the next request.
    Whole,
    /// The source could not go on. The head is already written, so the exchange closes the connection without ending
    /// the body, and the client sees it cut short.
    Failed,
    /// The exchange stopped the stream with `Enough`: the client hung up, the body stalled, or the response has no
    /// body to send, as one to `HEAD` does.
    Stopped,
}

/// How long a streamed response's body is, as its source knows before it sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.response_length")]
pub enum ResponseLength {
    /// Its length, written as its `Content-Length`: a source that sends more or fewer bytes closes the connection.
    Known(u64),
    /// Not known: the body is written in chunks to an HTTP/1.1 client, and to an HTTP/1.0 one until the connection
    /// closes.
    Unknown,
}
