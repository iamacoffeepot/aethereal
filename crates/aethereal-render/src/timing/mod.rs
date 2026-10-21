//! Timing: how long the GPU took over each pass of a frame, read from timestamps it writes as each pass begins and
//! ends, on a device that has them (ADR-0023).

#[cfg(test)]
mod tests;

use aethereal_data::{Kind, KindName, Plain};

/// Asks the renderer how long the GPU takes over each pass of the next frame it draws. Answered with [`Timed`] once
/// the GPU has drawn it and the host has mapped its timestamps back, a frame or more later; refused at once on a
/// device without timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct TimeFrame;

impl Kind for TimeFrame {
    const NAME: KindName = KindName::new("aether.render.time_frame");
}

/// What the renderer answers [`TimeFrame`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Timed {
    /// The nanoseconds from the beginning of each pass to its end, in the order they run: each pass of each frame
    /// that ends on a texture, in the order the frames run, then each pass of the window's main frame.
    Took {
        passes: Vec<u64>,
    },
    Refused(TimingRefused),
}

impl Kind for Timed {
    const NAME: KindName = KindName::new("aether.render.timed");
}

/// Why a frame was not timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum TimingRefused {
    /// The device has no timestamps: the GPU it was opened on does not offer WebGPU's `timestamp-query`.
    Unsupported,
    /// The GPU did not map the timestamps back.
    Unmapped,
}

impl Kind for TimingRefused {
    const NAME: KindName = KindName::new("aether.render.timing_refused");
}

/// The nanoseconds of each pass, from `stamps`, the timestamps the GPU resolved: a pass's beginning and its end after
/// each other, each eight bytes in the GPU's order of bytes, little-endian, and each tick `period` nanoseconds. A pass
/// whose end reads before its beginning, as a GPU's counter may when it is reset between them, took none.
pub(crate) fn passes(stamps: &[u8], period: f32) -> Vec<u64> {
    let ticks: Vec<u64> = stamps
        .chunks_exact(8)
        .map(|stamp| stamp.iter().rev().fold(0, |ticks, byte| (ticks << 8) | u64::from(*byte)))
        .collect();

    ticks.chunks_exact(2).map(|pass| nanos(pass[1].saturating_sub(pass[0]), period)).collect()
}

/// The nanoseconds of `ticked` ticks of `period` nanoseconds each.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pass's ticks are far within a float's whole numbers, and nanoseconds past a u64 saturate"
)]
fn nanos(ticked: u64, period: f32) -> u64 {
    (ticked as f64 * f64::from(period)) as u64
}
