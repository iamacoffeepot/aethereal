//! The HTTP capability (ADR-0035): one root, [`Http`], an HTTP/1.1 server on the TCP capability.
//!
//! An actor asks the root to [`Serve`] a port, at its typed path,
//! `path::<Http>(())`, naming a handler that takes [`Handling`], usually
//! itself, and is answered with a [`Server`] beneath the root. Each
//! [`Exchange`] beneath the server reads one connection `Tcp` accepted,
//! parses its requests, checking every limit before a request goes further,
//! asks the handler each [`Request`], and writes the [`Response`] it is
//! answered with.
//!
//! The root needs nothing of its host but `Tcp`, so it is bootstrapped for
//! every host. Its settings, the section `http`, are its config.

mod exchange;
mod http;
mod message;
mod server;
mod settings;
mod status;

pub use exchange::{Asking, Exchange, ExchangeConfig, Sending};
pub use http::{Binding, Http, Serve, Served};
pub use message::{Handling, Header, Method, Request, Response};
pub use server::{Server, ServerConfig};
pub use settings::{HttpSettings, Limits, ReadLimitTooLow};
pub use status::Status;
