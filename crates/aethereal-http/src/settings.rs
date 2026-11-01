//! The HTTP capability's settings (ADR-0028, ADR-0035).

use crate::Header;
use aethereal_data::Plain;
use core::error::Error;
use core::fmt;
use core::mem::size_of;

/// The HTTP capability's section, `http`: its limits on servers and on what each request may hold, and how long each
/// part of an exchange may take (ADR-0035). They are the root's config. Where listeners bind is `Tcp`'s setting.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.http.settings")]
#[settings]
pub struct HttpSettings {
    /// The most servers the root holds.
    #[flag]
    pub most_servers: u32,
    /// The most exchanges each server holds; past it, an accepted connection is shut at once.
    #[flag]
    pub most_exchanges: u32,
    /// The most bytes of a request's head, past which it is answered `431`.
    #[flag]
    pub most_head_bytes: u32,
    /// The most bytes of a request's body, past which it is answered `413`.
    #[flag]
    pub most_body_bytes: u32,
    /// The most bytes of each chunk a connection reads for an exchange.
    #[flag]
    pub chunk_bytes: u32,
    /// The chunks an exchange's read stream is granted ahead.
    #[flag]
    pub window: u32,
    /// The milliseconds a request may take to arrive whole, from its first byte.
    #[flag]
    pub request_within_millis: u32,
    /// The milliseconds a connection may wait between one response and the next request.
    #[flag]
    pub idle_within_millis: u32,
    /// The milliseconds a handler may take to respond, past which the request is answered `504`.
    #[flag]
    pub answer_within_millis: u32,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            most_servers: 16,
            most_exchanges: 64,
            most_head_bytes: 16 * 1024,
            most_body_bytes: 8 * 1024 * 1024,
            chunk_bytes: 64 * 1024,
            window: 4,
            request_within_millis: 10_000,
            idle_within_millis: 60_000,
            answer_within_millis: 300_000,
        }
    }
}

/// The fewest bytes of a head that hold a header: `a:`, and its line's end.
const LEAST_HEADER_BYTES: usize = 4;

impl HttpSettings {
    /// The most bytes a turn reads into one piece of mail of a server of these settings: the larger of a chunk and a
    /// whole request, its head as the most headers that fit, as reading a list charges each of its elements whole.
    #[must_use]
    pub fn most_read_bytes(&self) -> usize {
        let size = |bytes: u32| usize::try_from(bytes).unwrap_or(usize::MAX);
        let head = size(self.most_head_bytes);
        let headers = (head / LEAST_HEADER_BYTES).saturating_mul(size_of::<Header>());
        let request = head.saturating_add(headers).saturating_add(size(self.most_body_bytes));

        request.max(size(self.chunk_bytes))
    }
}

/// Why `Http` did not start: mail it would send could not be read, which would let a client end its handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimitTooLow {
    /// The most bytes a turn reads into one piece of mail.
    pub read_limit: usize,
    /// What the settings need of it.
    pub needed: usize,
}

impl fmt::Display for ReadLimitTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { read_limit, needed } = self;
        write!(f, "the engine's `read_limit` is {read_limit}, and `http`'s settings need {needed}")
    }
}

impl Error for ReadLimitTooLow {}

/// What every exchange of a server holds to: the settings that bound one request (ADR-0035).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.limits")]
pub struct Limits {
    pub most_head_bytes: u32,
    pub most_body_bytes: u32,
    pub window: u32,
    pub request_within_millis: u32,
    pub idle_within_millis: u32,
    pub answer_within_millis: u32,
}

impl HttpSettings {
    /// The limits each exchange holds to.
    #[must_use]
    pub const fn limits(&self) -> Limits {
        Limits {
            most_head_bytes: self.most_head_bytes,
            most_body_bytes: self.most_body_bytes,
            window: self.window,
            request_within_millis: self.request_within_millis,
            idle_within_millis: self.idle_within_millis,
            answer_within_millis: self.answer_within_millis,
        }
    }
}
