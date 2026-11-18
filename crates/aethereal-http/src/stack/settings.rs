//! A stack's settings (ADR-0028, ADR-0035): where it listens, and what each of its connections holds to.

use crate::http1::fit::{BlobBytesTooLow, ReadLimitTooLow, head_read_bytes, size};
use aethereal_actor::Root;
use aethereal_data::Plain;
use aethereal_substrate::{Bootstrapping, Host, Unbuilt};
use aethereal_tcp::Interface;

/// A stack's settings, its own section, which its `Bootstrap` names (ADR-0029): where it listens, and what each of its
/// connections holds to. A program with two stacks has two sections of it, so it suggests no flags, which two sections
/// would both suggest.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.http.stack_settings")]
#[settings]
pub struct StackSettings {
    /// The port it listens on: 0 for any free port.
    pub port: u16,
    /// The interface `Tcp` binds it on.
    pub interface: Interface,
    /// The most connections it holds open: past it, an accepted connection is shut at once.
    pub most_connections: u32,
    /// The most bytes of a request's head, past which it is answered `431`, and of a chunk's size line or a trailer's
    /// field, past which it is answered `413`.
    pub most_head_bytes: u32,
    /// The most bytes of each chunk a connection reads: the `Chunks` framing's `most_bytes`.
    pub chunk_bytes: u32,
    /// The chunks each connection's read stream is granted ahead.
    pub window: u32,
    /// The milliseconds a head may take to arrive whole, from its first byte.
    pub head_within_millis: u32,
    /// The milliseconds a body may go without moving: bytes coming, or its handler asking for it.
    pub stall_within_millis: u32,
    /// The milliseconds a connection may wait between one response and the next request.
    pub idle_within_millis: u32,
    /// The milliseconds the handler may take to answer once the body is read, or will not be, past which the request
    /// is answered `504`.
    pub answer_within_millis: u32,
}

impl Default for StackSettings {
    fn default() -> Self {
        Self {
            port: 0,
            interface: Interface::Settings,
            most_connections: 64,
            most_head_bytes: 16 * 1024,
            chunk_bytes: 64 * 1024,
            window: 4,
            head_within_millis: 10_000,
            stall_within_millis: 60_000,
            idle_within_millis: 60_000,
            answer_within_millis: 300_000,
        }
    }
}

impl StackSettings {
    /// The most bytes a turn reads into one piece of mail of these settings: a request's head, as the most headers that
    /// fit. A body and each chunk read are blobs, which no mail's limit weighs (ADR-0042).
    #[must_use]
    pub fn most_read_bytes(&self) -> usize {
        head_read_bytes(self.most_head_bytes)
    }

    /// Whether the engine `cx` bootstraps on can carry what a stack of these settings takes in, as the stack's
    /// `Bootstrap` checks before it begins: a head of `most_head_bytes` is mail its handler reads within the engine's
    /// `read_limit`, and a chunk a connection reads is a blob within its `blob_bytes`. A body gathered whole is as
    /// large as its handler asks, which is its handler's to fit.
    ///
    /// # Errors
    ///
    /// [`ReadLimitTooLow`] or [`BlobBytesTooLow`], as the program's failure to start.
    pub fn fit<H: Host, R: Root>(&self, cx: &Bootstrapping<'_, H, R>) -> Result<(), Unbuilt> {
        let read_limit = cx.read_limit();
        let needed = self.most_read_bytes();
        if read_limit < needed {
            return Err(Unbuilt::Failed(Box::new(ReadLimitTooLow { read_limit, needed })));
        }
        let blob_bytes = cx.blob_bytes();
        let needed = size(self.chunk_bytes);
        if blob_bytes < needed {
            return Err(Unbuilt::Failed(Box::new(BlobBytesTooLow { blob_bytes, needed })));
        }

        Ok(())
    }
}
