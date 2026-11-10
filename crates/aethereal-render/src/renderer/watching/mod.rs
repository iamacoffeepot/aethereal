//! Who the renderer tells of each frame it draws, and what it counts for them (ADR-0053): the subscribers of each
//! frame's statistics, and the bytes written to the GPU between frames, counted only while there is one; what each
//! thing it makes weighs of the GPU's memory, which it totals always; the subscribers of the GPU's time over each
//! frame's passes; and the adapter it draws on, answered to whoever asks.

#[cfg(test)]
mod tests;

use super::walk::Walk;
use super::{Instances, Kept, Last, Mesh, Origin, Renderer, Storage, Texture, Uniforms, Windowed};
use crate::made::Weighs;
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
    /// memory and the count of each thing it made, and how full the atlas is, copied from the totals it keeps as each
    /// thing is made, made again, and released, walking nothing.
    pub(super) fn frame_stats(&self, elapsed_nanos: u64, budget: FrameBudget, drawn: FrameDrawn) -> FrameStats {
        let (atlas, atlas_bytes) = self.text.atlas();
        let mut memory = GpuMemory { atlas_bytes, ..self.windows_memory };
        for made in [
            self.meshes.memory(),
            self.textures.memory(),
            self.instances.memory(),
            self.uniforms.memory(),
            self.storage.memory(),
            self.frames.memory(),
        ] {
            memory.add(made);
        }
        let alive = Alive {
            meshes: self.meshes.alive(),
            textures: self.textures.alive(),
            programs: self.programs.alive(),
            placements: self.placements.alive().saturating_add(self.text.placed.alive()),
            texts: self.text.made.alive(),
        };

        FrameStats { elapsed_nanos, budget, drawn, memory, alive, atlas }
    }
}

/// A mesh holds its vertices and its indices.
impl Weighs for Mesh {
    fn memory(&self) -> GpuMemory {
        GpuMemory { meshes_bytes: self.vertices.size() + self.indices.size(), ..GpuMemory::default() }
    }
}

/// A texture made of pixels is a texture; one a frame ends on is a render target.
impl Weighs for Texture {
    fn memory(&self) -> GpuMemory {
        let bytes = stats::texture_bytes(&self.made);
        match self.origin {
            Origin::Pixels => GpuMemory { textures_bytes: bytes, ..GpuMemory::default() },
            Origin::Frame(_) => GpuMemory { targets_bytes: bytes, ..GpuMemory::default() },
        }
    }
}

impl Weighs for Instances {
    fn memory(&self) -> GpuMemory {
        GpuMemory { buffers_bytes: self.buffer.size(), ..GpuMemory::default() }
    }
}

impl Weighs for Uniforms {
    fn memory(&self) -> GpuMemory {
        GpuMemory { buffers_bytes: self.buffer.size(), ..GpuMemory::default() }
    }
}

impl Weighs for Storage {
    fn memory(&self) -> GpuMemory {
        GpuMemory { buffers_bytes: self.buffer.size(), ..GpuMemory::default() }
    }
}

/// A frame holds its targets.
impl Weighs for Walk {
    fn memory(&self) -> GpuMemory {
        GpuMemory { targets_bytes: self.targets_bytes(), ..GpuMemory::default() }
    }
}

/// A window holds the renderer's own main frame's targets and the copy of its last frame.
impl Weighs for Windowed {
    fn memory(&self) -> GpuMemory {
        GpuMemory { targets_bytes: self.own.targets_bytes() + self.last.bytes(), ..GpuMemory::default() }
    }
}

impl Last {
    /// The bytes of the copy kept: none while none is.
    pub(super) fn bytes(&self) -> u64 {
        match self {
            Self::Undrawn => 0,
            Self::Kept(kept) => stats::texture_bytes(kept),
        }
    }
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
