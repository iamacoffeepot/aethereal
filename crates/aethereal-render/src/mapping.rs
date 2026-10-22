//! What the renderer asks its host for, and what the host brings back: a buffer the GPU copied into, mapped for
//! reading, which the GPU does after the turn that asked, and the host waits on (ADR-0019).

use aethereal_data::Plain;

/// A buffer the renderer asks its host to map for reading, pushed onto the renderer's controller (ADR-0019). The
/// chassis takes it between steps and starts it; once the GPU has mapped the buffer, it pushes what became of it,
/// [`Mapped`], onto the renderer's source.
pub struct Mapping {
    read: u64,
    buffer: wgpu::Buffer,
}

impl Mapping {
    /// The ask to map `buffer`, for the read `read`.
    pub(crate) const fn new(read: u64, buffer: wgpu::Buffer) -> Self {
        Self { read, buffer }
    }

    /// Starts mapping the buffer, and calls `done` with what became of it once the GPU has mapped it: for the host to
    /// call between steps, and to push what `done` is given onto the renderer's source. Nothing waits: in a browser the
    /// page's own promise calls `done`, and a native host polls its device between steps without waiting.
    pub fn start(self, done: impl FnOnce(Mapped) + Send + 'static) {
        let read = self.read;
        self.buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let mapped = match result {
                Ok(()) => Mapped::Ready { read },
                Err(_) => Mapped::Failed { read },
            };
            done(mapped);
        });
    }
}

/// What became of a buffer the renderer asked its host to map, brought from its source to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.mapped")]
pub enum Mapped {
    /// The buffer of the read `read` is mapped, and its bytes can be read.
    Ready { read: u64 },
    /// The GPU did not map it: its device was lost, or it was dropped.
    Failed { read: u64 },
}
