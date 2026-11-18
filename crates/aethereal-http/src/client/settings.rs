//! The HTTP capability's settings (ADR-0028, ADR-0035): the client's.

use crate::http1::fit::{head_read_bytes, size};
use aethereal_data::Plain;

/// The HTTP capability's section, `http`: its limits on clients and on what each response may hold (ADR-0035). They
/// are the root's config. A stack's settings are its own section.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.http.settings")]
#[settings]
pub struct HttpSettings {
    /// The most clients the root holds.
    #[flag]
    pub most_clients: u32,
    /// The most bytes of each chunk a client's connection reads.
    #[flag]
    pub chunk_bytes: u32,
    /// The chunks a client's read stream is granted ahead.
    #[flag]
    pub window: u32,
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
            most_clients: 64,
            chunk_bytes: 64 * 1024,
            window: 4,
            most_reply_head_bytes: 16 * 1024,
            most_reply_body_bytes: 8 * 1024 * 1024,
        }
    }
}

impl HttpSettings {
    /// The most bytes a turn reads into one piece of mail of these settings: a response's head, as the most headers
    /// that fit, as reading a list charges each of its elements whole. A body and each chunk read are blobs, which no
    /// mail's limit weighs (ADR-0042).
    #[must_use]
    pub fn most_read_bytes(&self) -> usize {
        head_read_bytes(self.most_reply_head_bytes)
    }

    /// The most bytes of blobs a response to a client of these settings holds at once: its whole body, and the chunk
    /// read beside it (ADR-0042).
    #[must_use]
    pub fn most_blob_bytes(&self) -> usize {
        size(self.most_reply_body_bytes).saturating_add(size(self.chunk_bytes))
    }

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

/// What every client holds to: the settings that bound one response (ADR-0035).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.client_limits")]
pub struct ClientLimits {
    pub most_head_bytes: u32,
    pub most_body_bytes: u32,
    pub window: u32,
}
