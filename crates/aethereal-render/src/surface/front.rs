//! The front the chassis hands the render root each frame through, one for every window, and what the render root asks
//! of each window's frame it drew (ADR-0019, ADR-0021).

use aethereal_actor::Resource;
use aethereal_window::WindowId;
use crossbeam_queue::{ArrayQueue, SegQueue};
use std::sync::Arc;

/// The render root's end of the front its chassis hands it each frame through: a slot for each window, keyed by the
/// window's id, each holding at most one frame, the texture of that window's surface to draw this cycle into (ADR-0019:
/// a value bound to the host stays with the chassis, and the capability holds a front for it). The chassis puts each
/// window's frame in before the cycle, and the render root takes them at `Render`.
///
/// It is one front for every window, and not one for each: a resource the render root keeps leaves it only when it
/// ends, so a front handed for each window would stay kept after its window closed. Its slots come and go as the
/// chassis opens and closes windows, between steps, through its end of the front, [`Fronting`] (ADR-0027).
pub struct Front {
    slots: Vec<(WindowId, Slot)>,
    changes: Arc<SegQueue<Change>>,
}

/// The chassis's end of the front: it adds a window's slot as it makes the window's surface, and takes it out as it
/// drops the surface, each between steps. The render root's end sees each change as it next takes its frames, in
/// the order made, so it never meets a slot half made, and neither end waits on the other.
pub struct Fronting {
    changes: Arc<SegQueue<Change>>,
}

/// One window's slot: at most one frame.
type Slot = Arc<ArrayQueue<wgpu::Texture>>;

/// A change the chassis made to the front's slots.
enum Change {
    /// The slot of `window`, added in place of one it had.
    Added(WindowId, Slot),
    /// The slot of `window`, taken out.
    Removed(WindowId),
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
    /// A front with no window's slot in it, and the chassis's end of it, which adds each window's slot as the chassis
    /// makes the window's surface.
    #[must_use]
    pub fn new() -> (Self, Fronting) {
        let changes = Arc::new(SegQueue::new());

        (Self { slots: Vec::new(), changes: Arc::clone(&changes) }, Fronting { changes })
    }

    /// The frame of each window to draw this cycle, taken, beside the window, once the slots the chassis added and
    /// took out since the last are added and taken out: none for a window the chassis handed none, as when its
    /// surface gave no texture or it is hidden or of no size.
    pub(crate) fn take(&mut self) -> Vec<(WindowId, wgpu::Texture)> {
        // Bounded by what was changed before this began, as a chassis changes the slots only between steps.
        let changed = self.changes.len();
        for change in (0..changed).map_while(|_| self.changes.pop()) {
            match change {
                Change::Added(window, slot) => {
                    self.slots.retain(|(slotted, _)| *slotted != window);
                    self.slots.push((window, slot));
                }
                Change::Removed(window) => self.slots.retain(|(slotted, _)| *slotted != window),
            }
        }

        self.slots.iter().filter_map(|(window, slot)| Some((*window, slot.pop()?))).collect()
    }
}

impl Resource for Front {}

impl Fronting {
    /// The chassis's end of the slot of `window`, added here, in place of one it had. Each slot is shared by the
    /// chassis's thread, which puts a frame between cycles, and the worker that runs the render root's `Render`, so it
    /// is a lock-free queue that holds one: what one mail at a time orders is the render root's turns, not the host's.
    pub(crate) fn hand(&self, window: WindowId) -> Handing {
        let slot = Arc::new(ArrayQueue::new(1));
        self.changes.push(Change::Added(window, Arc::clone(&slot)));

        Handing { slot }
    }

    /// Takes the slot of `window` out of the front, as the chassis drops the window's surface: the render root is
    /// handed nothing for it from its next take.
    pub fn remove(&self, window: WindowId) {
        self.changes.push(Change::Removed(window));
    }
}

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
