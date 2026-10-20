//! The time a step carries: the host's, as it began the step (ADR-0011, ADR-0018).

use aethereal_data::Plain;
use core::ops::Add;
use core::time::Duration;

/// A moment on the host's clock: nanoseconds since a moment of the host's choosing, the clock its timers run on
/// (ADR-0018).
///
/// The engine reads no clock (ADR-0001). The host passes a step its time as it
/// begins it, and every turn of the step reads that one time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Plain)]
pub struct Time {
    nanos: u64,
}

impl Time {
    /// The host's first moment: the time before the host has passed one in.
    pub const ZERO: Self = Self { nanos: 0 };

    /// The moment `nanos` nanoseconds after the host's first.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    /// How many nanoseconds after the host's first moment it is.
    #[must_use]
    pub const fn nanos(self) -> u64 {
        self.nanos
    }
}

/// The moment `later` after this one, or the last the host's clock can say when that is past it.
impl Add<Duration> for Time {
    type Output = Self;

    fn add(self, later: Duration) -> Self {
        let later = u64::try_from(later.as_nanos()).unwrap_or(u64::MAX);

        Self { nanos: self.nanos.saturating_add(later) }
    }
}
