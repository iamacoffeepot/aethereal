//! Who the renderer tells of each frame it draws, and what it counts for them (ADR-0053): the subscribers of each
//! frame's statistics, and the bytes written to the GPU between frames, counted only while there is one; the
//! subscribers of the GPU's time over each frame's passes; and the adapter it draws on, answered to whoever asks.

#[cfg(test)]
mod tests;

use super::walk::Walk;
use super::{Kept, Last, Origin, Renderer};
use crate::stats::{
    self, Alive, DescribeGpu, FrameBudget, FrameDrawn, FrameStats, GpuDescribed, GpuMemory, GpuTimes,
    SubscribeFrameStats, SubscribeGpuTimes, Tally, UnsubscribeFrameStats, UnsubscribeGpuTimes,
};
use aethereal_actor::{Ended, Fault, Handles, Level, ProtocolRef, Subscribers, ToAny, ToSender, Turn};
use aethereal_data::Bound;
use std::mem;

/// Who is sent each frame's statistics, and what is counted for them between frames.
#[derive(Bound)]
pub(super) enum Watch {
    /// No one: nothing is counted.
    Unwatched,
    /// These subscribers, never none, and the bytes written to the GPU since the last frame drawn, or since the first
    /// subscribed.
    Watched { subscribers: Subscribers<(FrameStats,)>, uploaded_bytes: u64 },
}

impl Watch {
    /// Adds `subscriber`, watched through `turn`, once however often it asks. The first begins the count of bytes
    /// written from nothing.
    fn subscribe<S>(&mut self, turn: &mut Turn<'_, Renderer, S>, subscriber: ProtocolRef<(FrameStats,)>) {
        let unwatched = matches!(self, Self::Unwatched);
        if unwatched {
            *self = Self::Watched { subscribers: Subscribers::new(), uploaded_bytes: 0 };
        }
        if let Self::Watched { subscribers, .. } = self {
            subscribers.subscribe(turn, subscriber);
        }
    }

    /// Removes a subscriber by `remove`, which answers whether it held one: the last removed stops the counting.
    fn remove(&mut self, remove: impl FnOnce(&mut Subscribers<(FrameStats,)>) -> bool) {
        let Self::Watched { subscribers, .. } = self else {
            return;
        };
        let removed = remove(subscribers);
        let none = subscribers.references().is_empty();
        if removed && none {
            *self = Self::Unwatched;
        }
    }

    /// Takes in that the actor `ended` has ended: when it was a subscriber, it is no longer.
    pub(super) fn ended(&mut self, ended: &Ended) {
        self.remove(|subscribers| subscribers.ended(ended));
    }

    /// Counts `bytes` written to the GPU, while watched.
    pub(super) fn uploaded(&mut self, bytes: usize) {
        if let Self::Watched { uploaded_bytes, .. } = self {
            *uploaded_bytes = uploaded_bytes.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        }
    }

    /// What a frame drawn now counts into: nothing while unwatched; while watched, its draws, beginning from the bytes
    /// written since the last, which are counted again from nothing.
    pub(super) fn tally(&mut self) -> Tally {
        match self {
            Self::Unwatched => Tally::Off,
            Self::Watched { uploaded_bytes, .. } => {
                let uploaded_bytes = mem::take(uploaded_bytes);
                Tally::On(FrameDrawn { draws: 0, triangles: 0, uploaded_bytes })
            }
        }
    }
}

impl Kept {
    /// The statistics of the frame of the cycle at `elapsed_nanos`, aimed at `budget`, which drew `drawn`: the GPU
    /// memory and the count of each thing it made, and how full the atlas is, as it holds them now.
    pub(super) fn frame_stats(&self, elapsed_nanos: u64, budget: FrameBudget, drawn: FrameDrawn) -> FrameStats {
        let (atlas, atlas_bytes) = self.text.atlas();
        let (textures_bytes, ends_bytes) = self.textures_bytes();
        let memory = GpuMemory {
            meshes_bytes: self.meshes.values().map(|mesh| mesh.vertices.size() + mesh.indices.size()).sum(),
            textures_bytes,
            targets_bytes: self.targets_bytes() + ends_bytes,
            buffers_bytes: self.buffers_bytes(),
            atlas_bytes,
        };
        let alive = Alive {
            meshes: count(self.meshes.values()),
            textures: count(self.textures.values()),
            programs: count(self.programs.values()),
            placements: count(self.placements.values()) + count(self.text.placed.values()),
            texts: count(self.text.made.values()),
        };

        FrameStats { elapsed_nanos, budget, drawn, memory, alive, atlas }
    }

    /// The bytes of the textures made of pixels, and of those frames end on.
    fn textures_bytes(&self) -> (u64, u64) {
        let (mut pixels, mut ends) = (0, 0);
        for texture in self.textures.values() {
            let bytes = stats::texture_bytes(&texture.made);
            match texture.origin {
                Origin::Pixels => pixels += bytes,
                Origin::Frame(_) => ends += bytes,
            }
        }

        (pixels, ends)
    }

    /// The bytes of each frame's targets, each window's own main frame's, and the copy of each window's last frame.
    fn targets_bytes(&self) -> u64 {
        let frames: u64 = self.frames.values().map(Walk::targets_bytes).sum();
        let windows: u64 =
            self.windows.iter().map(|windowed| windowed.own.targets_bytes() + windowed.last.bytes()).sum();

        frames + windows
    }

    /// The bytes of the instances, uniforms, and storage made.
    fn buffers_bytes(&self) -> u64 {
        let instances: u64 = self.instances.values().map(|made| made.buffer.size()).sum();
        let uniforms: u64 = self.uniforms.values().map(|made| made.buffer.size()).sum();
        let storage: u64 = self.storage.values().map(|made| made.buffer.size()).sum();

        instances + uniforms + storage
    }
}

impl Last {
    /// The bytes of the copy kept: none while none is.
    fn bytes(&self) -> u64 {
        match self {
            Self::Undrawn => 0,
            Self::Kept(kept) => stats::texture_bytes(kept),
        }
    }
}

/// How many `made` holds, as a count of things alive, far below what a `u32` counts.
fn count<T>(made: impl Iterator<Item = T>) -> u32 {
    u32::try_from(made.count()).unwrap_or(u32::MAX)
}

/// Adds the sender, which must handle [`FrameStats`], to those sent each frame's statistics: from the next frame
/// drawn, the renderer counts what it draws and writes for them.
impl Handles<SubscribeFrameStats> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: SubscribeFrameStats, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(subscriber) = turn.cast::<(FrameStats,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle `FrameStats` asked to be sent them");
            return Ok(());
        };
        self.watch.subscribe(turn, subscriber);

        Ok(())
    }
}

/// Removes the sender from those sent each frame's statistics: once none is left, nothing is counted.
impl Handles<UnsubscribeFrameStats> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: UnsubscribeFrameStats, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender();
        self.watch.remove(|subscribers| subscribers.unsubscribe(sender));

        Ok(())
    }
}

/// Adds the sender, which must handle [`GpuTimes`], to those sent each frame's pass times, from the next frame drawn;
/// on a device without timestamps it is told so, and not added.
impl Handles<SubscribeGpuTimes> for Renderer {
    type Sends = (ToAny<(GpuTimes,), GpuTimes>,);

    fn handle(&mut self, _: SubscribeGpuTimes, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(subscriber) = turn.cast::<(GpuTimes,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle `GpuTimes` asked to be sent them");
            return Ok(());
        };
        let features = turn.open(&mut self.gpu)?.device().features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            turn.to(subscriber).send(&GpuTimes::Unsupported)?;
            return Ok(());
        }
        self.timed.subscribe(turn, subscriber);

        Ok(())
    }
}

/// Removes the sender from those sent each frame's pass times: once none is left, and no one asked [`TimeFrame`],
/// no frame is timed.
///
/// [`TimeFrame`]: crate::timing::TimeFrame
impl Handles<UnsubscribeGpuTimes> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: UnsubscribeGpuTimes, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender();
        self.timed.unsubscribe(sender);

        Ok(())
    }
}

/// Answers the adapter the renderer draws on.
impl Handles<DescribeGpu> for Renderer {
    type Sends = (ToSender<GpuDescribed>,);

    fn handle(&mut self, _: DescribeGpu, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let described = GpuDescribed::of(&turn.open(&mut self.gpu)?.device().adapter_info());
        turn.reply(&described)?;

        Ok(())
    }
}
