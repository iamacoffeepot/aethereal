//! The HTTP capability (ADR-0035): one root, [`Http`], an HTTP/1.1 server and client on the TCP capability.
//!
//! An actor asks the root to [`Serve`] a port, at its typed path,
//! `path::<Http>(())`, naming a handler that takes [`Handling`], usually
//! itself, and is answered with a [`Server`] beneath the root. Each
//! [`Exchange`] beneath the server reads one connection `Tcp` accepted,
//! parses its requests, checking every limit before a request goes further,
//! asks the handler each [`Request`], and writes the [`Response`] it is
//! answered with.
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

mod client;
mod exchange;
mod head;
mod http;
mod message;
mod server;
mod settings;
mod status;

pub use client::{
    Client, ClientConfig, Failure, Fetch, Fetched, Finished, Follow, Limit, Outgoing, Received, Receiving, Reply,
    ReplyHead, Requesting, Switch, Unwritable,
};
pub use exchange::{Asking, Deadline, Exchange, ExchangeConfig, Sending};
pub use http::{Binding, Connect, Connected, Http, Peer, Reaching, ReachingUnix, Serve, Served, Unreached};
pub use message::{Handling, Header, Method, Request, Response};
pub use server::{Server, ServerConfig};
pub use settings::{BlobBytesTooLow, ClientLimits, HttpSettings, Limits, ReadLimitTooLow};
pub use status::Status;
