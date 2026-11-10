//! Frame statistics (ADR-0053): what the renderer tells its subscribers of each frame it draws, and of the GPU's time
//! over its passes, sent only while someone subscribes, so a program no one watches spends nothing on them; and the
//! adapter it draws on, answered once.
//!
//! A subscriber of [`FrameStats`] is sent them after each `Render`: of one that drew, its draws and triangles, counted
//! as the frame was recorded, and the bytes written to the GPU since the last; of one that drew nothing, as when no
//! window is drawn, that it did not; and the GPU memory of what the renderer made,
//! how many of each thing are alive, and how full the glyph atlas is, which the renderer keeps as running totals,
//! subscribed to or not, changed where each thing is made, made again at another size, and released, so a frame
//! copies them and walks nothing. A subscriber of [`GpuTimes`] is sent each frame's pass
//! times as the GPU's timestamps are mapped back, a frame or more later, each beside its pass's name; on a device
//! without timestamps it is told so once, as it subscribes, and kept no further.

mod budget;
#[cfg(test)]
mod tests;
#[cfg(feature = "tools")]
pub mod tool;

pub use budget::{FALLBACK_REFRESH_MILLIHERTZ, FrameBudget, refresh_nanos};

use crate::frame::Frame;
use crate::pass::PassName;
use aethereal_data::Plain;

/// Asks the renderer to send this actor [`FrameStats`] after each `Render` that draws, until it unsubscribes or ends:
/// the sender must handle `FrameStats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.subscribe_frame_stats")]
pub struct SubscribeFrameStats;

/// Asks the renderer to send this actor no more [`FrameStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.unsubscribe_frame_stats")]
pub struct UnsubscribeFrameStats;

/// Asks the renderer to send this actor [`GpuTimes`] for each frame it draws, until it unsubscribes or ends: the
/// sender must handle `GpuTimes`. On a device without timestamps it is sent [`GpuTimes::Unsupported`] at once, and
/// nothing after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.subscribe_gpu_times")]
pub struct SubscribeGpuTimes;

/// Asks the renderer to send this actor no more [`GpuTimes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.unsubscribe_gpu_times")]
pub struct UnsubscribeGpuTimes;

/// What the renderer tells its subscribers of a cycle's frame: the cycle's `elapsed_nanos`, as its `Tick` and `Render`
/// have it; the frame time the program aims at; what the frame drew and wrote, or that it drew nothing; the GPU memory
/// of what the renderer made; how many of each thing are alive; and how full the glyph atlas is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.frame_stats")]
pub struct FrameStats {
    pub elapsed_nanos: u64,
    pub budget: FrameBudget,
    pub drawn: FrameDrawing,
    pub memory: GpuMemory,
    pub alive: Alive,
    pub atlas: AtlasFill,
}

/// Whether a cycle drew a frame, and what it drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.frame_drawing")]
pub enum FrameDrawing {
    /// It drew this.
    Drawn(FrameDrawn),
    /// It drew nothing, as no window was handed a frame: none shown, or none the compositor asked a frame of. Its
    /// cycle is no frame's time, and a chart of frame times leaves it out; the bytes written since the last frame
    /// drawn are counted into the next that is.
    Undrawn,
}

/// What a frame drew and wrote: each draw it recorded, placements', texts', and effects'; the triangles of those whose
/// count the CPU knows, as a draw whose count the GPU reads from storage is counted as a draw, its triangles not; and
/// the bytes written to the GPU since the frame before, of what actors made and wrote and of the glyphs written into
/// the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.render.frame_drawn")]
pub struct FrameDrawn {
    pub draws: u32,
    pub triangles: u64,
    pub uploaded_bytes: u64,
}

/// The GPU memory of what the renderer made, in bytes, by what it holds, as the renderer counts the buffers and
/// textures it made rather than what the driver reserved for them: meshes' vertices and indices; textures made of
/// pixels; render targets, which are each frame's targets, the textures frames end on, and the copy of each window's
/// last frame; instances, uniforms, and storage; and the glyph atlas. A texture is counted at its first level, each of
/// its samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.render.gpu_memory")]
pub struct GpuMemory {
    pub meshes_bytes: u64,
    pub textures_bytes: u64,
    pub targets_bytes: u64,
    pub buffers_bytes: u64,
    pub atlas_bytes: u64,
}

impl GpuMemory {
    /// Counts `more` in, each kind into its own.
    pub(crate) const fn add(&mut self, more: Self) {
        self.meshes_bytes = self.meshes_bytes.saturating_add(more.meshes_bytes);
        self.textures_bytes = self.textures_bytes.saturating_add(more.textures_bytes);
        self.targets_bytes = self.targets_bytes.saturating_add(more.targets_bytes);
        self.buffers_bytes = self.buffers_bytes.saturating_add(more.buffers_bytes);
        self.atlas_bytes = self.atlas_bytes.saturating_add(more.atlas_bytes);
    }

    /// Counts `gone` out, each kind out of its own.
    pub(crate) const fn remove(&mut self, gone: Self) {
        self.meshes_bytes = self.meshes_bytes.saturating_sub(gone.meshes_bytes);
        self.textures_bytes = self.textures_bytes.saturating_sub(gone.textures_bytes);
        self.targets_bytes = self.targets_bytes.saturating_sub(gone.targets_bytes);
        self.buffers_bytes = self.buffers_bytes.saturating_sub(gone.buffers_bytes);
        self.atlas_bytes = self.atlas_bytes.saturating_sub(gone.atlas_bytes);
    }
}

/// How many of each thing the renderer made are alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.render.alive")]
pub struct Alive {
    pub meshes: u32,
    pub textures: u32,
    pub programs: u32,
    pub placements: u32,
    pub texts: u32,
}

/// How full the glyph atlas is: the texels its glyphs' fields cover, its side now, and the longest side it may grow
/// to, each in texels. When a field fits nowhere at the longest side, it is packed again from nothing (ADR-0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.render.atlas_fill")]
pub struct AtlasFill {
    pub used_texels: u64,
    pub side: u32,
    pub most_side: u32,
}

/// The GPU's time over a frame's passes, as its timestamps are mapped back, a frame or more after it was drawn.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.gpu_times")]
pub enum GpuTimes {
    /// The frame of the cycle at `elapsed_nanos`: each pass it ran, in the order it ran them, as [`Timed::Took`]
    /// orders them, each beside its frame and its name.
    ///
    /// [`Timed::Took`]: crate::timing::Timed::Took
    Took { elapsed_nanos: u64, passes: Vec<PassTime> },
    /// The device has no timestamps: sent once, as the subscriber subscribes, and nothing after.
    Unsupported,
}

/// One pass of a frame the GPU ran: the frame it is of; its name, as a frame's passes are named by a type (ADR-0025);
/// its label, the name of that type's kind, such as `aether.render.main`, for a person to read; and the nanoseconds
/// from its beginning to its end.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.pass_time")]
pub struct PassTime {
    pub frame: Frame,
    pub pass: PassName,
    pub label: String,
    pub nanos: u64,
}

/// A pass a frame ran, before the GPU's time over it is mapped back: its frame, its name, and its label.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ran {
    pub frame: Frame,
    pub pass: PassName,
    pub label: String,
}

/// Asks the renderer which adapter it draws on: answered with [`GpuDescribed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.describe_gpu")]
pub struct DescribeGpu;

/// The adapter the renderer draws on, as wgpu reports it: its name, its kind, the backend it is driven through, and
/// its driver and the driver's own description of itself, either empty where the platform does not say.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.gpu_described")]
pub struct GpuDescribed {
    pub name: String,
    pub device_type: String,
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
}

impl GpuDescribed {
    /// The adapter `info` describes.
    #[must_use]
    pub fn of(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            device_type: format!("{:?}", info.device_type),
            backend: String::from(info.backend.to_str()),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
        }
    }
}

/// Each pass's nanoseconds, `nanos`, in the order the passes ran, beside the pass it is of, `named`, in the same order:
/// what [`GpuTimes::Took`] holds. A pass with no time, as when fewer were mapped back than were run, is left out, and
/// so is a time with no pass.
pub(crate) fn named(named: Vec<Ran>, nanos: &[u64]) -> Vec<PassTime> {
    named
        .into_iter()
        .zip(nanos)
        .map(|(Ran { frame, pass, label }, &nanos)| PassTime { frame, pass, label, nanos })
        .collect()
}

/// What a frame's recording counts: nothing, while no one is sent [`FrameStats`], or each draw it records.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tally {
    /// No one is sent them: nothing is counted.
    Off,
    /// Each draw recorded is counted into this.
    On(FrameDrawn),
}

impl Tally {
    /// Counts one draw of `triangles`, when it counts.
    pub(crate) const fn draw(&mut self, triangles: u64) {
        if let Self::On(drawn) = self {
            drawn.draws += 1;
            drawn.triangles += triangles;
        }
    }
}

/// The triangles of `count` instances of a mesh drawn with `indices` indices, as a list of triangles.
pub(crate) fn triangles(indices: u32, count: u32) -> u64 {
    u64::from(indices / 3) * u64::from(count)
}

/// The bytes of `texture`'s first level, each of its samples: its width, height, and layers, by its format's bytes
/// a texel. A format whose texels are not counted alone, a depth and stencil together, is counted at four.
pub(crate) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let size = texture.size();
    let texel = texture.format().block_copy_size(None).unwrap_or(4);
    let texels = u64::from(size.width) * u64::from(size.height) * u64::from(size.depth_or_array_layers);

    texels * u64::from(texel) * u64::from(texture.sample_count())
}
