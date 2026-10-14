//! The clock a host hands the engine (ADR-0001).

/// A clock the host hands the engine, read to measure how long turns take (ADR-0001, ADR-0011).
///
/// The engine never reads a clock of its own: a browser host's clock is not
/// the native one. Workers read it at once, so it is `Sync`.
pub trait Clock: Sync {
    /// Nanoseconds since a moment of the host's choosing. It never goes back.
    fn nanos(&self) -> u64;
}
