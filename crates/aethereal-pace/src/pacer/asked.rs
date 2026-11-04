//! What an actor asks the pacer, to hold the program's clock, let it run, or advance it while held, and how it is
//! answered (ADR-0045).

use aethereal_data::Plain;

/// How the program's clock moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub enum Pace {
    /// With the host's time: the host says when each cycle is due, and each `Tick`'s delta is its time between them.
    #[default]
    Real,
    /// Only when it is advanced: no cycle is due of the host's own accord.
    Held,
}

/// Asks the pacer to set the clock to `pace`: answered with [`PaceSet`] in the turn that asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.set_pace")]
pub struct SetPace {
    pub pace: Pace,
}

/// The clock is set as asked: `was` is how it moved before, so the asker can set it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.pace_set")]
pub struct PaceSet {
    pub was: Pace,
}

/// Asks the pacer to run `cycles` cycles of a held clock, one after another, each with a `Tick` of exactly
/// `delta_nanos`: answered with [`Advanced`] once the last has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.advance")]
pub struct Advance {
    pub cycles: u32,
    pub delta_nanos: u64,
}

/// What became of an advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.advanced")]
pub enum Advanced {
    /// Every cycle asked for has finished, and the program's time, the sum of every `Tick`'s delta, is this.
    Advanced { elapsed_nanos: u64 },
    /// It was not run, or not to its end, for this reason.
    Refused(Unadvanced),
}

/// Why an advance was not run, or not to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Unadvanced {
    /// The clock runs: it is held first, so a running program is never frozen as a side effect. An advance asked of a
    /// held clock that is set running before its cycles have run is answered so too.
    Running,
    /// It asked for no cycle.
    NoCycles,
    /// It asked for a delta of no time.
    NoTime,
    /// It asked for a delta past the most the engine gives one `Tick`, this many nanoseconds, which no actor is
    /// written for.
    PastLimit { limit_nanos: u64 },
    /// The program is stopping: no cycle runs again.
    Stopping,
}

/// Asks the pacer how the clock moves, answered with [`PaceRead`] in the turn that asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.read_pace")]
pub struct ReadPace;

/// How the clock moves: as it was set, a stopping program's as held, as no cycle of the host's own ends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.pace_read")]
pub struct PaceRead {
    pub pace: Pace,
}
