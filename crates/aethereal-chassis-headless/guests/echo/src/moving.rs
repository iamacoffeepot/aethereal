//! Guests a test moves onto new code (ADR-0054): the echo is built a second time with the `echo_v2` cfg set, and a test
//! publishes that build as their kinds' head. Each answers a probe with the version of the code that handled it and
//! what its state holds, so a test sees which code ran a turn and what the move carried. When each was asked whether
//! to move, and what it answered, the test reads in the engine's record of what it ran.

use crate::Shut;
use aethereal_actor::{
    Actor, Beginning, Bytes, Carries, ChildOf, Ending, Fault, Handles, Portability, ToSender, Top, Turn, Upgrade,
    Upgrades,
};
#[cfg(echo_v2)]
use aethereal_actor::{Portable, TakeOverError};
use aethereal_data::{Bound, Plain};

/// The version of the code this build is: two when built with the `echo_v2` cfg.
const VERSION: u64 = if cfg!(echo_v2) {
    2
} else {
    1
};

/// What a mover is made from: whether it holds, answering `Later` while it does.
#[derive(Plain)]
#[kind(name = "aether.guest.mover_config")]
pub struct MoverConfig {
    pub holds: bool,
}

/// A probe, which is answered with a report.
#[derive(Plain)]
#[kind(name = "aether.guest.probe")]
pub struct Probe;

/// A blob for a mover to keep a reference to.
#[derive(Bound)]
#[kind(name = "aether.guest.keep")]
pub struct Keep {
    pub blob: Bytes,
}

/// A word to stop holding, on which a mover answers `Now` from then on.
#[derive(Plain)]
#[kind(name = "aether.guest.let_go")]
pub struct LetGo;

/// What a probe is answered with: the version of the code that handled it; how many probes the actor has answered,
/// this one among them; whether its new code found a field absent as it took its state over; and how many bytes the
/// blobs it keeps hold, read through the references it keeps.
#[derive(Plain)]
#[kind(name = "aether.guest.report")]
pub struct Report {
    pub version: u64,
    pub count: u64,
    pub first_move: bool,
    pub kept: u64,
}

/// An actor whose second version changes its state's layout: its fields reordered, and one added that the first
/// lacks, which its `take_over` finds absent. It answers `Later` while it holds.
#[cfg(not(echo_v2))]
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.mover")]
pub struct Mover {
    count: u64,
    holds: bool,
    kept: Vec<Bytes>,
}

/// An actor whose second version changes its state's layout: its fields reordered, and one added that the first
/// lacks, which its `take_over` finds absent. It answers `Later` while it holds.
#[cfg(echo_v2)]
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.mover")]
pub struct Mover {
    kept: Vec<Bytes>,
    first_move: bool,
    holds: bool,
    count: u64,
}

impl Mover {
    /// A mover that has answered no probe and keeps nothing.
    #[cfg(not(echo_v2))]
    const fn new(holds: bool) -> Self {
        Self { count: 0, holds, kept: Vec::new() }
    }

    /// A mover that has answered no probe and keeps nothing.
    #[cfg(echo_v2)]
    const fn new(holds: bool) -> Self {
        Self { kept: Vec::new(), first_move: false, holds, count: 0 }
    }

    /// Whether its code found its `first_move` field absent as it took its state over: never, for the first version,
    /// which has no such field.
    #[cfg(not(echo_v2))]
    const fn first_move(&self) -> bool {
        false
    }

    /// Whether its code found its `first_move` field absent as it took its state over.
    #[cfg(echo_v2)]
    const fn first_move(&self) -> bool {
        self.first_move
    }
}

impl ChildOf for Mover {
    type Parent = Top;
}

impl Actor for Mover {
    type Config = MoverConfig;
    type Key = ();
    type Handles = (Probe, Keep, LetGo, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: MoverConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self::new(config.holds))
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Upgrades for Mover {
    fn upgrade(&self) -> Upgrade {
        if self.holds {
            Upgrade::Later
        } else {
            Upgrade::Now
        }
    }

    /// Its fields by name, wherever they stand; `first_move`, which the first version lacks, is true when absent.
    #[cfg(echo_v2)]
    fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
        let Some(count) = old.count else {
            return Err(TakeOverError::missing("count"));
        };
        let first_move = old.first_move.is_none();

        Ok(Self { kept: old.kept.unwrap_or_default(), first_move, holds: old.holds.unwrap_or(false), count })
    }
}

impl Carries<Probe> for Mover {
    type Answers = (Report,);
}

impl Handles<Probe> for Mover {
    type Sends = (ToSender<Report>,);

    fn handle(&mut self, _: Probe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.count += 1;
        let kept = self.kept.iter().map(|blob| u64::try_from(turn.read(blob).len()).unwrap_or(u64::MAX)).sum();
        let report = Report { version: VERSION, count: self.count, first_move: self.first_move(), kept };
        turn.reply(&report)?;

        Ok(())
    }
}

impl Handles<Keep> for Mover {
    type Sends = ();

    fn handle(&mut self, keep: Keep, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.kept.push(keep.blob);

        Ok(())
    }
}

impl Handles<LetGo> for Mover {
    type Sends = ();

    fn handle(&mut self, _: LetGo, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.holds = false;

        Ok(())
    }
}

impl Handles<Shut> for Mover {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// An actor whose second version keeps its state's layout, so its state is taken in as it is: it writes no
/// `take_over`, and a move that called one would fail.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.keeper")]
pub struct Keeper {
    count: u64,
}

impl ChildOf for Keeper {
    type Parent = Top;
}

impl Actor for Keeper {
    type Config = MoverConfig;
    type Key = ();
    type Handles = (Probe,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: MoverConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { count: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Upgrades for Keeper {
    fn upgrade(&self) -> Upgrade {
        Upgrade::Now
    }
}

impl Carries<Probe> for Keeper {
    type Answers = (Report,);
}

impl Handles<Probe> for Keeper {
    type Sends = (ToSender<Report>,);

    fn handle(&mut self, _: Probe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.count += 1;
        turn.reply(&Report { version: VERSION, count: self.count, first_move: false, kept: 0 })?;

        Ok(())
    }
}

/// An actor whose second version adds a field its `take_over` requires, which the first lacks: its move from the
/// first always fails.
#[cfg(not(echo_v2))]
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.stubborn")]
pub struct Stubborn {
    count: u64,
}

/// An actor whose second version adds a field its `take_over` requires, which the first lacks: its move from the
/// first always fails.
#[cfg(echo_v2)]
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.stubborn")]
pub struct Stubborn {
    count: u64,
    answered: u64,
}

impl Stubborn {
    /// One that has answered no probe.
    #[cfg(not(echo_v2))]
    const fn new() -> Self {
        Self { count: 0 }
    }

    /// One that has answered no probe.
    #[cfg(echo_v2)]
    const fn new() -> Self {
        Self { count: 0, answered: 0 }
    }

    /// Counts a probe answered.
    #[cfg(not(echo_v2))]
    const fn answer(&mut self) {
        self.count += 1;
    }

    /// Counts a probe answered, and one since it moved.
    #[cfg(echo_v2)]
    const fn answer(&mut self) {
        self.count += 1;
        self.answered += 1;
    }
}

impl ChildOf for Stubborn {
    type Parent = Top;
}

impl Actor for Stubborn {
    type Config = MoverConfig;
    type Key = ();
    type Handles = (Probe,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: MoverConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self::new())
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Upgrades for Stubborn {
    fn upgrade(&self) -> Upgrade {
        Upgrade::Now
    }

    /// Refused: `answered`, which the first version lacks, is required.
    #[cfg(echo_v2)]
    fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
        let Some(answered) = old.answered else {
            return Err(TakeOverError::missing("answered"));
        };

        Ok(Self { count: old.count.unwrap_or(0), answered })
    }
}

impl Carries<Probe> for Stubborn {
    type Answers = (Report,);
}

impl Handles<Probe> for Stubborn {
    type Sends = (ToSender<Report>,);

    fn handle(&mut self, _: Probe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.answer();
        turn.reply(&Report { version: VERSION, count: self.count, first_move: false, kept: 0 })?;

        Ok(())
    }
}
