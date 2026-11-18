//! What a stack asks its handler and is answered (ADR-0035): a request, typed by what its layers attached and by its
//! route's captures, and a response, its body blobs (ADR-0042).

use super::Gather;
use super::gather::Gathering;
use crate::http1::head::is_token;
use crate::stack::{BodyLength, InboundBody, ReadBody, ResponseLength, StreamedBody};
use crate::{Header, Method, Status};
use aethereal_actor::{Bytes, ProtocolRef, Streamed};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::SocketAddress;

/// A request a stack asks its handler, once every layer has passed it: typed by `A`, what the layers attached, and by
/// `C`, its route's captures, `()` where there is no router. Its credentials went to the layers, and are not here.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.stack.request")]
pub struct Request<A, C> {
    pub method: Method,
    /// The target's path, as sent.
    pub path: String,
    /// What follows the target's `?`, empty when there is none.
    pub query: String,
    /// In the order sent, duplicates kept, each name lowercased, but `Authorization`.
    pub headers: Vec<Header>,
    /// What the layers attached.
    pub attached: A,
    pub captures: C,
    /// The handle the body is read through.
    pub body: BodyHandle,
    /// Where the request came from.
    pub peer: SocketAddress,
}

impl<A, C> Request<A, C> {
    /// The value of the first header named `name`, lowercase: none when the request has none.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|header| header.name == name).map(|header| header.value.as_str())
    }
}

/// How a request's body is read: as a stream, opened on the stack's `reader`, or whole, by asking its `gatherer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
pub struct BodyHandle {
    /// `Known(n)` by its `Content-Length`, 0 for none; or `Chunked`.
    pub length: BodyLength,
    /// The stack, asked for the body as a stream.
    pub reader: ProtocolRef<Streamed<InboundBody>>,
    /// The stack, asked for the body whole.
    pub gatherer: ProtocolRef<Gathering>,
    /// Which of the stack's requests it is the body of.
    request: u64,
}

impl BodyHandle {
    /// The handle of the body of the stack's request numbered `request`, `length` long, read through `reader` and
    /// gathered through `gatherer`.
    pub(super) const fn new(
        length: BodyLength,
        reader: ProtocolRef<Streamed<InboundBody>>,
        gatherer: ProtocolRef<Gathering>,
        request: u64,
    ) -> Self {
        Self { length, reader, gatherer, request }
    }

    /// What an [`InboundBody`] stream on the `reader` opens with, to read the body as it arrives: a body of any size.
    #[must_use]
    pub const fn read(&self) -> ReadBody {
        ReadBody::new(self.request)
    }

    /// What the `gatherer` is asked for the body in one answer, refused past `most_bytes`.
    #[must_use]
    pub const fn whole(&self, most_bytes: u32) -> Gather {
        Gather::new(most_bytes, self.request)
    }
}

/// What a handler answers a request with, or a layer answers in its place. The stack writes `Content-Length`,
/// `Transfer-Encoding`, and `Connection` itself, and passes a whole body to its connection without reading it.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.response")]
pub struct Response {
    pub status: Status,
    pub headers: Vec<Header>,
    pub body: ResponseBody,
}

/// A response's body.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
pub enum ResponseBody {
    /// None: written with `Content-Length: 0`, and needing no blob, so a pure layer, which has no turn to put bytes,
    /// answers with it.
    Empty,
    /// Put whole, written after the head as it is, with its `Content-Length`.
    Whole(Bytes),
    /// Read from its source as the connection takes it, once the head is written: its length, when known, is named in
    /// the head as it passes the layers on the way out, as its `content-length`.
    Streamed(StreamedBody),
}

impl ResponseBody {
    /// The body's source and its number for it, when a source streams it.
    pub(super) const fn streamed(&self) -> Option<StreamedBody> {
        match self {
            Self::Streamed(streamed) => Some(*streamed),
            Self::Empty | Self::Whole(_) => None,
        }
    }
}

/// A response's head: its status and its headers, which a layer changes on the way out.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct ResponseHead {
    pub status: Status,
    pub headers: Vec<Header>,
}

impl Response {
    /// A response of `status` whose body is `body`, of the type `content_type`.
    #[must_use]
    pub fn new(status: Status, content_type: &str, body: Bytes) -> Self {
        Self { status, headers: vec![Header::new("content-type", content_type)], body: ResponseBody::Whole(body) }
    }

    /// A response of `status` with no header and no body.
    #[must_use]
    pub const fn empty(status: Status) -> Self {
        Self { status, headers: Vec::new(), body: ResponseBody::Empty }
    }

    /// `401`, with `WWW-Authenticate: <scheme> realm="<realm>"`, so a client that holds credentials sends them again.
    /// A scheme that is not a token, or a realm holding `"`, `\`, or a control byte, would write a header that does not
    /// parse: that is its author's fault, and the challenge is `500`.
    #[must_use]
    pub fn challenge(scheme: &str, realm: &str) -> Self {
        let is_scheme = !scheme.is_empty() && is_token(scheme.as_bytes());
        let is_quotable = realm.bytes().all(|byte| byte != b'"' && byte != b'\\' && !byte.is_ascii_control());
        let writes = is_scheme && is_quotable;
        if !writes {
            return Self::empty(Status::InternalServerError);
        }
        let challenge = Header::new("www-authenticate", &format!("{scheme} realm=\"{realm}\""));

        Self { status: Status::Unauthorized, headers: vec![challenge], body: ResponseBody::Empty }
    }

    /// `403`.
    #[must_use]
    pub const fn forbidden() -> Self {
        Self::empty(Status::Forbidden)
    }

    /// Its head and its body, apart. A streamed body's length is named in the head, as its `content-length` when it is
    /// known, in place of any the response had, so a layer that changes the body's length says so there on the way
    /// out, before any of the body passes it.
    pub(super) fn into_parts(self) -> (ResponseHead, ResponseBody) {
        let Self { status, mut headers, body } = self;
        if let ResponseBody::Streamed(StreamedBody { length, .. }) = body {
            headers.retain(|header| header.name != CONTENT_LENGTH);
            if let ResponseLength::Known(length) = length {
                headers.push(Header::new(CONTENT_LENGTH, &length.to_string()));
            }
        }

        (ResponseHead { status, headers }, body)
    }
}

/// The header a streamed body's length is named by in its response's head as the head passes the layers.
const CONTENT_LENGTH: &str = "content-length";

impl ResponseHead {
    /// The length its streamed body is framed by, its `content-length` taken out of its headers once every layer has
    /// passed it: one that reads as a number is the body's length, and none, or one that does not read, or more than
    /// one, leaves it unknown.
    pub(super) fn take_length(&mut self) -> ResponseLength {
        let mut named = self.headers.iter().filter(|header| header.name == CONTENT_LENGTH);
        let length = match (named.next(), named.next()) {
            (Some(header), None) => header.value.parse().map_or(ResponseLength::Unknown, ResponseLength::Known),
            (None | Some(_), _) => ResponseLength::Unknown,
        };
        self.headers.retain(|header| header.name != CONTENT_LENGTH);

        length
    }
}

#[cfg(test)]
mod tests {
    use super::{Response, ResponseBody};
    use crate::{Header, Status};

    #[test]
    fn a_challenge_names_its_scheme_and_realm_and_one_that_would_not_parse_is_a_fault() {
        // Catches a challenge written without its realm quoted, so git's credential helper never answers it, and a
        // realm whose quote or line ending would write a header of the author's own.
        let challenge = Response::challenge("Basic", "the hub");

        assert_eq!(challenge.status, Status::Unauthorized);
        assert_eq!(challenge.headers, [Header::new("www-authenticate", "Basic realm=\"the hub\"")]);
        assert_eq!(challenge.body, ResponseBody::Empty);
        assert_eq!(Response::challenge("Basic", "a \"quoted\" realm").status, Status::InternalServerError);
        assert_eq!(Response::challenge("Basic", "two\r\nlines").status, Status::InternalServerError);
        assert_eq!(Response::challenge("Two words", "the hub").status, Status::InternalServerError);
    }
}
