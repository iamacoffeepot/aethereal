//! The HTTP capability's settings (ADR-0028, ADR-0035).

use crate::Header;
use aethereal_data::Plain;
use core::error::Error;
use core::fmt;
use core::mem::size_of;

/// The HTTP capability's section, `http`: its limits on servers and on what each request may hold, how long each part
/// of an exchange may take, and its limits on clients and on what each response may hold (ADR-0035). They are the
/// root's config. Where listeners bind is `Tcp`'s setting.
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
    /// The milliseconds a handler may take to respond, past which the request is answered `504`: the deadline of the
    /// exchange's ask of its handler, which every request made for it inherits, at every hop (ADR-0041).
    #[flag]
    pub answer_within_millis: u32,
    /// The most clients the root holds.
    #[flag]
    pub most_clients: u32,
    /// The most bytes of a response's head a client reads, and of each line of a chunked body, past which the
    /// response fails as too large.
    #[flag]
    pub most_reply_head_bytes: u32,
    /// The most bytes of a response's body a client reads whole, past which the response fails as too large. A body
    /// streamed has no limit: it is never held whole.
    #[flag]
    pub most_reply_body_bytes: u32,
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
            most_clients: 64,
            most_reply_head_bytes: 16 * 1024,
            most_reply_body_bytes: 8 * 1024 * 1024,
        }
    }
}

/// The fewest bytes of a head that hold a header: `a:`, and its line's end.
const LEAST_HEADER_BYTES: usize = 4;

impl HttpSettings {
    /// The most bytes a turn reads into one piece of mail of these settings: a request's head or a response's,
    /// whichever may be larger, as the most headers that fit, as reading a list charges each of its elements whole. A
    /// body and each chunk read are blobs, which no mail's limit weighs (ADR-0042).
    #[must_use]
    pub fn most_read_bytes(&self) -> usize {
        let head = size(self.most_head_bytes.max(self.most_reply_head_bytes));
        let headers = (head / LEAST_HEADER_BYTES).saturating_mul(size_of::<Header>());

        head.saturating_add(headers)
    }

    /// The most bytes of blobs a request to a server, or a response to a client, of these settings holds at once: its
    /// whole body, whichever may be larger, and the chunk read beside it (ADR-0042).
    #[must_use]
    pub fn most_blob_bytes(&self) -> usize {
        size(self.most_body_bytes.max(self.most_reply_body_bytes)).saturating_add(size(self.chunk_bytes))
    }
}

/// `bytes` as a `usize`, or the most one holds.
fn size(bytes: u32) -> usize {
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

/// Why `Http` did not start: mail it would send could not be read, so a request within its limits would never reach
/// its handler, nor a response its asker (ADR-0017).
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

/// Why `Http` did not start: the engine's blobs could not hold a request's body, so a request within its limits would be
/// answered `503` (ADR-0014, ADR-0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobBytesTooLow {
    /// The most bytes the engine's blobs hold.
    pub blob_bytes: usize,
    /// What the settings need of it.
    pub needed: usize,
}

impl fmt::Display for BlobBytesTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { blob_bytes, needed } = self;
        write!(f, "the engine's `blob_bytes` is {blob_bytes}, and `http`'s settings need {needed}")
    }
}

impl Error for BlobBytesTooLow {}

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

/// What every client holds to: the settings that bound one response (ADR-0035).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.client_limits")]
pub struct ClientLimits {
    pub most_head_bytes: u32,
    pub most_body_bytes: u32,
    pub window: u32,
}

impl HttpSettings {
    /// The limits each client holds to.
    #[must_use]
    pub const fn client_limits(&self) -> ClientLimits {
        ClientLimits {
            most_head_bytes: self.most_reply_head_bytes,
            most_body_bytes: self.most_reply_body_bytes,
            window: self.window,
        }
    }
}
