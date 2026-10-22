//! The engine's section of the configuration, which every host reads (ADR-0028, ADR-0029).

use crate::{Engine, Setup};
use aethereal_data::Plain;
use core::num::{NonZeroU32, NonZeroUsize};

/// The name of the engine's section.
pub const ENGINE: &str = "engine";

/// The name of the host's own section: how the program is run.
pub const RUNNING: &str = "running";

/// The engine's settings: its sizes, and the threads that run its steps (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.engine.settings")]
#[settings]
pub struct EngineSettings {
    /// The threads that run a step, the host's own among them.
    #[flag(long = "workers", short = 'w')]
    pub workers: NonZeroU32,
    /// The parts the engine's views are kept in, rounded up to a power of two.
    #[flag]
    pub parts: NonZeroU32,
    /// The most bytes a turn reads into any one piece of mail.
    pub read_limit: u32,
    /// The bytes of arenas mail in flight is written to, shared equally among the writers.
    pub mail_bytes: u64,
    /// The most bytes the engine's blobs hold (ADR-0014).
    pub blob_bytes: u64,
    /// The fused hops each part runs in a step (ADR-0013): with none, every piece of mail is handled in a later step.
    #[flag]
    pub hops: u32,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            workers: NonZeroU32::MIN,
            parts: NonZeroU32::MIN,
            read_limit: 1024,
            mail_bytes: 1 << 20,
            blob_bytes: 1 << 20,
            hops: 0,
        }
    }
}

impl EngineSettings {
    /// The threads that run a step, as the host counts them: the most a `usize` holds, when it holds fewer.
    #[must_use]
    pub fn worker_threads(&self) -> NonZeroUsize {
        NonZeroUsize::try_from(self.workers).unwrap_or(NonZeroUsize::MAX)
    }

    /// The setup of an engine of these sizes, each the most a `usize` holds when it holds less.
    pub(super) fn setup(&self) -> Setup<()> {
        let parts = NonZeroUsize::try_from(self.parts).unwrap_or(NonZeroUsize::MAX);
        Engine::new(parts, size(self.read_limit), size(self.mail_bytes), size(self.blob_bytes), size(self.hops))
    }
}

/// `value` as a `usize`, or the most one holds.
fn size(value: impl TryInto<usize>) -> usize {
    value.try_into().unwrap_or(usize::MAX)
}
