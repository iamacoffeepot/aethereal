//! What a client is asked, and what it answers with (ADR-0035): a request whole, or a response streamed as it
//! arrives, and a connection switched to another protocol.

use super::Client;
use crate::{Header, Method};
use aethereal_actor::{Bytes, Carries, ProtocolRef, Stream};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{HungUp, Writing};

/// A request a client writes: its method, its target, its headers, and its body. The client writes `Host`,
/// `Content-Length`, `Connection`, and `Upgrade` itself, and refuses a request that names one of them.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.outgoing")]
pub struct Outgoing {
    pub method: Method,
    /// The target's path: `/`, and what follows it up to the query.
    pub path: String,
    /// What follows the target's `?`: empty for a target with no query, and then no `?` is written.
    pub query: String,
    /// In the order written.
    pub headers: Vec<Header>,
    /// Put by the asker: an empty blob for a request with no body.
    pub body: Bytes,
}

/// Asked of a client: write `request`, and answer with its response once its body is whole.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.fetch")]
pub struct Fetch {
    pub request: Outgoing,
}

/// Answered whole, it is answered once its response is read.
impl Carries<Fetch> for Client {
    type Answers = (Fetched,);
}

/// What a fetch came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.fetched")]
pub enum Fetched {
    /// The response, its body whole.
    Whole(Reply),
    /// Why there is none.
    Failed(Failure),
}

/// A response a client read whole.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.reply")]
pub struct Reply {
    /// Its status code, whichever it is: a client reads every status a server writes.
    pub status: u16,
    /// In the order sent, duplicates kept, each name lowercased.
    pub headers: Vec<Header>,
    /// Put by the client that read it: empty for a response with no body.
    pub body: Bytes,
}

/// A response's head, the first part of a response a client streams.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.reply_head")]
pub struct ReplyHead {
    pub status: u16,
    /// In the order sent, duplicates kept, each name lowercased.
    pub headers: Vec<Header>,
}

/// A client's stream of one response as it arrives (ADR-0017): opened with [`Follow`], its first part the response's
/// head and every later part its body's bytes, or, once the connection has switched protocols, the connection's own
/// bytes; ended with how it finished. The asker's credit paces it: the client reads its connection only while the
/// asker has credit, so a reader that falls behind holds the server back, and no body is ever held whole.
pub struct Receiving;

impl Stream for Receiving {
    type Open = Follow;
    type Part = Received;
    type Done = Finished;
}

/// What a client is asked to stream: the response to `request`, as `switch` says.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.follow")]
pub struct Follow {
    pub request: Outgoing,
    pub switch: Switch,
}

/// Whether a request asks to switch its connection to another protocol (RFC 9110 §7.8).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.switch")]
pub enum Switch {
    /// It does not: the response's body is streamed, and the client takes another request once it ends.
    Stay,
    /// It asks to switch to `protocol`, a token, written as `Connection: Upgrade` and `Upgrade: <protocol>`. A `101`, or
    /// a `2xx` whose body runs until the connection closes, as a server that takes over the connection answers, hands
    /// the asker the connection: its bytes come as parts, those that arrived with the head first, and it is written
    /// through the reference in [`Received::Switched`]. Any other response is streamed as with [`Switch::Stay`].
    To { protocol: String },
}

/// One part of a streamed response.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.received")]
pub enum Received {
    /// The response's head: every later part is its body's.
    Head(ReplyHead),
    /// The response's head, and the connection switched: every later part is the connection's own bytes, and
    /// `writing` writes to it and shuts it. HTTP is no longer read on it.
    Switched { head: ReplyHead, writing: ProtocolRef<Writing> },
    /// Bytes of the body, or of the connection once switched, in the order they came.
    Bytes(Bytes),
}

/// How a streamed response finished.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.finished")]
pub enum Finished {
    /// Its body ended where its framing said: the client takes another request.
    Whole,
    /// It ended with the connection, which hung up as `HungUp` says: the connection had switched, or the body was one
    /// that runs until the connection closes, or the asker stopped the stream. The client has closed.
    Closed(HungUp),
    /// Why it ended early.
    Failed(Failure),
}

/// Why a client answered no response.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.failure")]
pub enum Failure {
    /// A request is already out on the client: it takes one at a time.
    Busy,
    /// The request would not write, and nothing was sent.
    Unwritable(Unwritable),
    /// The connection holds too many bytes unsent to take the request, and nothing was sent.
    Full,
    /// The response does not parse, or breaks HTTP's rules. The client has closed.
    Malformed,
    /// The response is past a limit of the client's settings. The client has closed.
    TooLarge(Limit),
    /// The connection hung up before the response was whole, as `HungUp` says. The client has closed.
    HungUp(HungUp),
    /// The engine had no room to hold what the client wrote, and nothing was sent; or what it read, and the client has
    /// closed (ADR-0014).
    NoRoom,
}

/// Why a request would not write.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.unwritable")]
pub enum Unwritable {
    /// The path does not start with `/`, or the path or query holds a byte a target cannot.
    Target,
    /// The header `name` is one the client writes itself, its name is not a token, or its value holds a control byte.
    Header { name: String },
    /// The protocol to switch to is not a token.
    Protocol,
}

/// Which of a client's limits a response went past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.limit")]
pub enum Limit {
    /// Its head, or a line of its chunked body, is past `most_reply_head_bytes`.
    Head,
    /// Its body, read whole, is past `most_reply_body_bytes`.
    Body,
}
