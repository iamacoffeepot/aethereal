//! The front the chassis hands the render root each frame through, and what the render root asks of the frame it drew
//! (ADR-0019, ADR-0021).

use aethereal_actor::Resource;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

/// The render root's end of the front its chassis hands it each frame through: a slot that holds at most one frame,
/// the texture of the window's surface to draw this cycle into (ADR-0019: a value bound to the host stays with the
/// chassis, and the capability holds a front for it). The chassis puts the frame in before the cycle, and the render
/// root takes it at `Render`.
pub struct Front {
    slot: Arc<ArrayQueue<wgpu::Texture>>,
}

/// The chassis's end of the front, which it puts each frame it acquired into.
pub struct Handing {
    slot: Arc<ArrayQueue<wgpu::Texture>>,
}

/// What the render root asks of the frame it took from its front, pushed onto a controller it owns once the frame is
/// drawn and submitted: that the chassis present it, after the cycle.
pub struct Present;

impl Front {
    /// A front, and the chassis's end of it. The slot is shared by the chassis's thread, which puts a frame between
    /// cycles, and the worker that runs the render root's `Render`, so it is a lock-free queue that holds one: what
    /// one mail at a time orders is the render root's turns, not the host's.
    pub(crate) fn new() -> (Self, Handing) {
        let slot = Arc::new(ArrayQueue::new(1));

        (Self { slot: Arc::clone(&slot) }, Handing { slot })
    }

    /// The frame to draw this cycle, taken: none when the chassis handed none, as when its surface gave no texture or
    /// its window is hidden or of no size.
    pub(crate) fn take(&self) -> Option<wgpu::Texture> {
        self.slot.pop()
    }
}

impl Resource for Front {}

impl Handing {
    /// Puts `frame` in the front, in place of one there that the render root did not take.
    pub fn put(&self, frame: wgpu::Texture) {
        drop(self.slot.force_push(frame));
    }

    /// Takes back a frame the render root did not take, as the chassis drops the frame it kept for it.
    pub fn clear(&self) {
        drop(self.slot.pop());
    }
}
