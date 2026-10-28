//! The HTTP capability (ADR-0035): one root, [`Http`], an HTTP/1.1 server on the TCP capability.
//!
//! An actor asks the root to [`Serve`] a port, at its typed path,
//! `path::<Http>(())`, and is answered with a [`Server`] beneath the root.
//! It opens a [`Requests`] stream on that server, whose credit is how many
//! requests it takes at once, and answers each [`Incoming`] request by
//! sending [`Respond`] to the [`Exchange`] it came on. Each exchange reads one
//! connection `Tcp` accepted, parses its requests, checking every limit
//! before a request goes further, and writes the responses.
//!
//! The root needs nothing of its host but `Tcp`, so it is bootstrapped for
//! every host. Its settings, the section `http`, are its config.

mod exchange;
mod http;
mod message;
mod server;
mod settings;
mod status;

pub use exchange::{Exchange, ExchangeConfig, Respond, Sending};
pub use http::{Binding, Http, Serve, Served};
pub use message::{Header, Method, Request, Response};
pub use server::{Incoming, Parsed, Requests, Server, ServerConfig, Stopped, Take};
pub use settings::{HttpSettings, Limits};
pub use status::Status;
