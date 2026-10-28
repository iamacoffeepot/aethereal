//! A request and its response, as plain data (ADR-0035).

use crate::Status;
use aethereal_data::Plain;
use aethereal_tcp::SocketAddress;

/// The methods a server takes. A request of any other is answered `501` before it reaches the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.method")]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

impl Method {
    /// The method `token` names: none when it is not one a server takes.
    #[must_use]
    pub fn named(token: &[u8]) -> Option<Self> {
        let method = match token {
            b"GET" => Self::Get,
            b"HEAD" => Self::Head,
            b"POST" => Self::Post,
            b"PUT" => Self::Put,
            b"DELETE" => Self::Delete,
            b"PATCH" => Self::Patch,
            b"OPTIONS" => Self::Options,
            _ => return None,
        };

        Some(method)
    }
}

/// One header: its name, lowercased in a request, and its value.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.header")]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    /// The header `name: value`.
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        Self { name: String::from(name), value: String::from(value) }
    }
}

/// A request an exchange parsed: whole, its body read, and every limit checked.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.request")]
pub struct Request {
    pub method: Method,
    /// The target's path, as sent.
    pub path: String,
    /// What follows the target's `?`, empty when there is none.
    pub query: String,
    /// In the order sent, duplicates kept, each name lowercased.
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    /// Where the request came from.
    pub peer: SocketAddress,
}

impl Request {
    /// The value of the first header named `name`, lowercase: none when the request has none.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|header| header.name == name).map(|header| header.value.as_str())
    }
}

/// What a handler answers a request with. The exchange writes `Content-Length` and `Connection` itself.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.response")]
pub struct Response {
    pub status: Status,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl Response {
    /// A response of `status`, whose body is `body`, of the type `content_type`.
    #[must_use]
    pub fn new(status: Status, content_type: &str, body: Vec<u8>) -> Self {
        Self { status, headers: vec![Header::new("content-type", content_type)], body }
    }

    /// A response of `status` with no body.
    #[must_use]
    pub const fn empty(status: Status) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new() }
    }
}
