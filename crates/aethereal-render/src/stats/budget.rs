//! The frame time a program aims at (ADR-0053: Frame statistics), which its chassis knows from how it presents and the
//! display it presents to, and hands the renderer through its front.

use aethereal_data::Plain;

/// The refresh a chassis assumes of a display that reports none, in millihertz: 60 Hz, the most common.
pub const FALLBACK_REFRESH_MILLIHERTZ: u32 = 60_000;

/// The frame time a program aims at, in nanoseconds: a limit its frames are held to, or a reference they are drawn
/// against but not held to, as when frames are presented as fast as they are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.frame_budget")]
pub enum FrameBudget {
    /// The frames are paced to it: by a cap, or by the display's refresh.
    Limit { nanos: u64 },
    /// The frames are not paced to it, and it is the display's refresh, for reference.
    Reference { nanos: u64 },
}

impl FrameBudget {
    /// The budget before the chassis has said one: the fallback refresh, as a reference.
    pub const UNTOLD: Self = Self::Reference { nanos: refresh_nanos(FALLBACK_REFRESH_MILLIHERTZ) };

    /// Its nanoseconds, a limit or a reference.
    #[must_use]
    pub const fn nanos(self) -> u64 {
        match self {
            Self::Limit { nanos } | Self::Reference { nanos } => nanos,
        }
    }
}

/// The nanoseconds of one frame of a display refreshed `millihertz` times a thousand seconds: a second over the
/// refresh, rounded down. A refresh of none is the fallback's.
#[must_use]
pub const fn refresh_nanos(millihertz: u32) -> u64 {
    let millihertz = if millihertz == 0 {
        FALLBACK_REFRESH_MILLIHERTZ
    } else {
        millihertz
    };

    1_000_000_000_000 / millihertz as u64
}
