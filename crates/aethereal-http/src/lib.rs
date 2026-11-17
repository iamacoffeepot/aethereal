//! The HTTP capability (ADR-0035): one root, [`Http`], an HTTP/1.1 server and client on the TCP capability.
//!
//! An actor asks the root to [`Serve`] a port, at its typed path,
//! `path::<Http>(())`, naming a handler that takes [`Handling`], usually
//! itself, and is answered with a [`Server`] beneath the root. Each
//! [`Exchange`] beneath the server reads one connection `Tcp` accepted,
//! parses its requests, checking every limit before a request goes further,
//! asks the handler each [`Request`], and writes the [`Response`] it is
//! answered with: whole, or streamed from a source the response names as
//! the connection takes it. A request under one of the serve's [`Mount`]s is asked of
//! the mount's handler as a [`StreamedRequest`] instead, its body streamed to
//! the handler as it arrives, paced by the handler's credit.
//!
//! An actor asks the root to [`Connect`] to a server, over TCP or to a Unix
//! socket, and is answered with a [`Client`] beneath the root, on one
//! connection. Asked a [`Fetch`], the client writes the request and answers
//! with the response whole; opened a [`Receiving`] stream, it streams the
//! response as it arrives, paced by its asker's credit, or hands over the
//! connection once it switches protocols. Every limit on a response is checked
//! before it goes further.
//!
//! The root needs nothing of its host but `Tcp`, so it is bootstrapped for
//! every host. Its settings, the section `http`, are its config.
//!
//! A program serves a port with a [`stack`] of its own instead: an actor that
//! listens through `Tcp` itself, and passes each request through its layers
//! to its handler.

pub mod base64;
mod body;
mod client;
mod exchange;
mod head;
mod http;
mod message;
mod mount;
mod pooled;
mod request_head;
mod response_head;
mod router;
mod server;
mod settings;
mod source;
pub mod stack;
mod status;

pub use client::{
    Client, ClientConfig, Failure, Fetch, Fetched, Finished, Follow, Limit, Outgoing, Received, Receiving, Reply,
    ReplyHead, Requesting, Switch, Unwritable,
};
pub use exchange::{Asking, Deadline, Exchange, ExchangeConfig, Sending};
pub use http::{Binding, Connect, Connected, Http, Peer, Reaching, ReachingUnix, Serve, Served, Unreached};
pub use message::{Handling, Header, Method, Request, Response, ResponseBody, StreamedBody};
pub use mount::{BodyBytes, BodyLength, BodyRead, Mount, ReadBody, RequestBody, StreamHandling, StreamedRequest};
pub use router::{Captures, Unroutable};
pub use server::{Server, ServerConfig};
pub use settings::{BlobBytesTooLow, ClientLimits, HttpSettings, Limits, ReadLimitTooLow};
pub use source::{ResponseBytes, ResponseLength, ResponseSource, SendBody, SourceEnd};
pub use status::Status;
