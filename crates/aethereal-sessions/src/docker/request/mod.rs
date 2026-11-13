//! The requests the Docker client writes (ADR-0057), each built from what it was asked, at the API version it pins.

#[cfg(test)]
mod tests;

use super::spec::strings;
use super::{Attach, ContainerId, Create, Events, Inspect, Kill, List, Logs, Remove, Start, Stop, Until, Unwritable};
use aethereal_data::{Plain, write_json_string};
use aethereal_http::Method;
use core::fmt::Write as _;

/// The version of Docker's Engine API every request names in its path, so a daemon answers each in the shapes this
/// client reads, whatever version it speaks itself.
///
/// It is 1.44: everything the client writes and reads is in that version. A daemon answers every version from its own
/// back to its minimum, and daemons raise that minimum over time, so the pin sits above the oldest versions still
/// answered, which go first, and below what current daemons speak.
pub const API_VERSION: &str = "1.44";

/// The nanoseconds of a second.
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// A request the client writes, before its body is put as a blob: its method, its path and query, and its body.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Call {
    pub method: Method,
    /// Under the pinned version: `/v1.44/containers/create`.
    pub path: String,
    /// Each value percent-encoded; empty for none.
    pub query: String,
    pub body: Body,
}

/// What a request's body is.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Body {
    Empty,
    /// JSON, written with its `Content-Type`.
    Json(String),
}

impl Create {
    /// `POST /containers/create?name=…`, with the spec's body.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let body = self.spec.body()?;

        Ok(Call::new(Method::Post, "/containers/create", query(&[("name", &self.spec.name)]), Body::Json(body)))
    }
}

impl Start {
    /// `POST /containers/{id}/start`.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        Ok(Call::new(Method::Post, &container(&self.container, "/start")?, String::new(), Body::Empty))
    }
}

impl Stop {
    /// `POST /containers/{id}/stop?t=…`.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let grace = self.grace_secs.to_string();

        Ok(Call::new(Method::Post, &container(&self.container, "/stop")?, query(&[("t", &grace)]), Body::Empty))
    }
}

impl Kill {
    /// `POST /containers/{id}/kill`, whose signal is SIGKILL unless named.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        Ok(Call::new(Method::Post, &container(&self.container, "/kill")?, String::new(), Body::Empty))
    }
}

impl Remove {
    /// `DELETE /containers/{id}?force=1&v=1`.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let path = container(&self.container, "")?;

        Ok(Call::new(Method::Delete, &path, query(&[("force", "1"), ("v", "1")]), Body::Empty))
    }
}

impl Inspect {
    /// `GET /containers/{id}/json`.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        Ok(Call::new(Method::Get, &container(&self.container, "/json")?, String::new(), Body::Empty))
    }
}

impl List {
    /// `GET /containers/json?all=1&filters=…`, filtered by the label.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let filters = filters(&[("label", &[&self.label.filter()?])]);

        Ok(Call::new(Method::Get, "/containers/json", query(&[("all", "1"), ("filters", &filters)]), Body::Empty))
    }
}

impl Attach {
    /// `POST /containers/{id}/attach?stream=1&stdin=1&stdout=1&stderr=1`, which takes the connection over.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let path = container(&self.container, "/attach")?;
        let streams = query(&[("stream", "1"), ("stdin", "1"), ("stdout", "1"), ("stderr", "1")]);

        Ok(Call::new(Method::Post, &path, streams, Body::Empty))
    }
}

impl Logs {
    /// `GET /containers/{id}/logs?stdout=1&stderr=1&since=…&follow=…`, `since` in seconds and nanoseconds.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let path = container(&self.container, "/logs")?;
        let since = format!("{}.{:09}", self.since_nanos / NANOS_PER_SEC, self.since_nanos % NANOS_PER_SEC);
        let follow = match self.until {
            Until::Now => "0",
            Until::Stopped => "1",
        };
        let asked = query(&[("stdout", "1"), ("stderr", "1"), ("since", &since), ("follow", follow)]);

        Ok(Call::new(Method::Get, &path, asked, Body::Empty))
    }
}

impl Events {
    /// `GET /events?filters=…`, filtered to containers that carry the label starting, dying, and running out of
    /// memory.
    pub(super) fn call(&self) -> Result<Call, Unwritable> {
        let label = self.label.filter()?;
        let filters = filters(&[("type", &["container"]), ("event", &["start", "die", "oom"]), ("label", &[&label])]);

        Ok(Call::new(Method::Get, "/events", query(&[("filters", &filters)]), Body::Empty))
    }
}

impl Call {
    /// A request of `method` to `path` under the pinned version.
    fn new(method: Method, path: &str, query: String, body: Body) -> Self {
        Self { method, path: format!("/v{API_VERSION}{path}"), query, body }
    }
}

/// The path of the container's `endpoint`: `/containers/{id}{endpoint}`.
fn container(container: &ContainerId, endpoint: &str) -> Result<String, Unwritable> {
    Ok(format!("/containers/{}{endpoint}", container.segment()?))
}

/// A query of `pairs`, each value percent-encoded.
fn query(pairs: &[(&str, &str)]) -> String {
    let mut query = String::new();
    for (place, (name, value)) in pairs.iter().enumerate() {
        if place > 0 {
            query.push('&');
        }
        query.push_str(name);
        query.push('=');
        encode(value, &mut query);
    }

    query
}

/// Docker's `filters`: a JSON object of each filter's name and the values it takes.
fn filters(named: &[(&str, &[&str])]) -> String {
    let mut written = String::from("{");
    for (place, (name, values)) in named.iter().enumerate() {
        if place > 0 {
            written.push(',');
        }
        write_json_string(name, &mut written);
        written.push(':');
        strings(values.iter().copied(), &mut written);
    }
    written.push('}');

    written
}

/// Appends `value` percent-encoded (RFC 3986 §2.1): every byte but the unreserved ones written as `%` and two hex
/// digits.
fn encode(value: &str, out: &mut String) {
    for byte in value.bytes() {
        let is_unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
}
