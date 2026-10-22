//! The front the chassis hands the render root each frame through, one for every window, and what the render root asks
//! of each window's frame it drew (ADR-0019, ADR-0021).

use aethereal_actor::Resource;
use aethereal_window::WindowId;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

/// The render root's end of the front its chassis hands it each frame through: a slot for each window, keyed by the
/// window's id, each holding at most one frame, the texture of that window's surface to draw this cycle into (ADR-0019:
/// a value bound to the host stays with the chassis, and the capability holds a front for it). The chassis puts each
/// window's frame in before the cycle, and the render root takes them at `Render`.
///
/// It is one front for every window, and not one for each: a resource the render root keeps leaves it only when it
/// ends, so a front handed for each window would stay kept after its window closed.
pub struct Front {
    slots: Vec<(WindowId, Arc<ArrayQueue<wgpu::Texture>>)>,
}

/// The chassis's end of one window's slot in the front, which it puts each frame it acquired of that window into.
pub struct Handing {
    slot: Arc<ArrayQueue<wgpu::Texture>>,
}

/// What the render root asks of the frame of `window` it took from its front, pushed onto a controller it owns once
/// the frame is drawn and submitted: that the chassis present it, after the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Present {
    window: WindowId,
}

impl Front {
    /// A front with no window's slot in it, for the chassis to add each window's to as it makes the window's surface.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// The chassis's end of the slot of `window`, made here, in place of one it had. Each slot is shared by the
    /// chassis's thread, which puts a frame between cycles, and the worker that runs the render root's `Render`, so it
    /// is a lock-free queue that holds one: what one mail at a time orders is the render root's turns, not the host's.
    pub(crate) fn hand(&mut self, window: WindowId) -> Handing {
        let slot = Arc::new(ArrayQueue::new(1));
        self.slots.retain(|(slotted, _)| *slotted != window);
        self.slots.push((window, Arc::clone(&slot)));

        Handing { slot }
    }

    /// The frame of each window to draw this cycle, taken, beside the window: none for a window the chassis handed
    /// none, as when its surface gave no texture or it is hidden or of no size.
    pub(crate) fn take(&self) -> Vec<(WindowId, wgpu::Texture)> {
        self.slots.iter().filter_map(|(window, slot)| Some((*window, slot.pop()?))).collect()
    }
}

impl Default for Front {
    fn default() -> Self {
        Self::new()
    }
}

impl Resource for Front {}

impl Handing {
    /// Puts `frame` in the window's slot, in place of one there that the render root did not take.
    pub fn put(&self, frame: wgpu::Texture) {
        drop(self.slot.force_push(frame));
    }

    /// Takes back a frame the render root did not take, as the chassis drops the frame it kept for it.
    pub fn clear(&self) {
        drop(self.slot.pop());
    }
}

impl Present {
    /// The ask that the chassis present the frame of `window`.
    pub(crate) const fn of(window: WindowId) -> Self {
        Self { window }
    }

    /// Whether any of `asked` is for the frame of `window`.
    pub(crate) fn asked_for(asked: &[Self], window: WindowId) -> bool {
        asked.iter().any(|present| present.window == window)
    }
}
