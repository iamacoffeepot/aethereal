//! The HTTP capability (ADR-0035): the pieces a program serves HTTP/1.1 from, on the TCP capability, and one root,
//! [`Http`], the client.
//!
//! A program serves a port with a [`stack`] of its own: an actor that listens through `Tcp` itself, reads and writes
//! every connection, and passes each request through its layers to its handler, or to a [`Router`] that picks the
//! handler by method and path.
//!
//! An actor asks the root to [`Connect`] to a server, over TCP or to a Unix socket, and is answered with a [`Client`]
//! beneath the root, on one connection. Asked a [`Fetch`], the client writes the request and answers with the response
//! whole; opened a [`Receiving`] stream, it streams the response as it arrives, paced by its asker's credit, or hands
//! over the connection once it switches protocols. Every limit on a response is checked before it goes further.
//!
//! The root needs nothing of its host but `Tcp`, so it is bootstrapped for every host. Its settings, the section
//! `http`, are its config.
//!
//! What the stack and the client share of HTTP/1.1, the reading of heads and bodies and the methods, headers, and
//! statuses, is in one module of its own.

pub mod base64;
mod client;
mod http1;
mod router;
pub mod stack;

pub use client::{
    Client, ClientConfig, ClientLimits, Connect, Connected, Failure, Fetch, Fetched, Finished, Follow, Http,
    HttpSettings, Limit, Outgoing, Peer, Reaching, ReachingUnix, Received, Receiving, Reply, ReplyHead, Requesting,
    Switch, Unreached, Unwritable,
};
pub use http1::fit::{BlobBytesTooLow, ReadLimitTooLow};
pub use http1::{Header, Method, Status};
pub use router::{Captures, Route, Router, Routes, Unroutable};
