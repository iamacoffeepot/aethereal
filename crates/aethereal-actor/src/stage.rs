//! The stages of a program's lifecycle, which the engine runs and sends its actors (ADR-0020).

use crate::{Actors, Arena, Fault, Handled, Item, Payload, Role};
use aethereal_data::{Decode, Encode, EncodeError, Kind, KindId, Plain};
use alloc::vec::Vec;
use core::marker::PhantomData;

/// A stage of the program's lifecycle: a kind the engine sends every actor that subscribed to it, and waits, before the
/// next, until every turn it set off has run (ADR-0020).
///
/// The engine makes the stage's value itself, with [`Stage::made`]: a stage
/// holds nothing but what the cycle's time gives it, and most hold nothing.
/// A chassis's crate may define a stage of its own, which its cycle runs
/// wherever the chassis puts it.
pub trait Stage: Kind + Encode<Actors> + Decode<Actors> + Default + 'static {
    /// Where the stage may stand: at an end of the lifecycle, among the shared stages in their order, or anywhere
    /// in a cycle, as a chassis's own stage.
    const PLACE: Place = Place::Own;

    /// The stage's value for a cycle at `time`.
    #[must_use]
    fn made(_time: Cycled) -> Self {
        Self::default()
    }
}

/// The stage `K`, as an actor handles it: what it lists in its `Handles` for each stage it may subscribe to, and whose
/// handler is handed the stage (ADR-0020).
///
/// It says which of the kinds a type handles are stages, so a type made where its engine does not run one is refused
/// before any of its code runs, and a turn subscribes only to a stage its actor's type handles.
pub struct On<K>(PhantomData<fn() -> K>);

impl<K: Stage> Handled for On<K> {
    const ROLE: Role = Role::Stage(KindId::of::<K>());
    type Item<'a> = K;

    fn take(item: Item<'_>) -> Result<K, Fault> {
        let Item::Mail(mail) = item else {
            unreachable!("only the stage is handed to its handler");
        };

        Ok(mail.contents()?)
    }
}

/// Where a stage may stand in the program's lifecycle (ADR-0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// At an end: `Start` or `Stop`, which no cycle holds.
    End,
    /// One of the stages the engine shares, which a cycle holds in the order of this rank.
    Shared(u8),
    /// A chassis's own, which a cycle may hold anywhere.
    Own,
}

/// The time of a cycle, as the engine sends it in [`Tick`]: how long since the last, and the time elapsed, both in
/// nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cycled {
    pub delta_nanos: u64,
    pub elapsed_nanos: u64,
}

/// The first stage, sent once, when the host starts the engine: what every actor made by then subscribed to it is
/// sent before anything the program runs.
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.start")]
pub struct Start;

impl Stage for Start {
    const PLACE: Place = Place::End;
}

/// The last stage, sent once, when the program is to stop: once every turn it set off has run, and every subscriber
/// has answered it or let it go, the host drops the engine.
///
/// It is the one stage sent as a request, answered with [`Drained`]. A subscriber with work in flight to finish holds
/// it ([`Turn::hold`](crate::Turn::hold)) and answers once that work is done, and the program stops only after
/// (ADR-0020).
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.stop")]
pub struct Stop;

impl Stage for Stop {
    const PLACE: Place = Place::End;
}

/// What a subscriber answers [`Stop`] with: it has finished the work it had in flight (ADR-0020).
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.drained")]
pub struct Drained;

/// The stage in which actors update what they hold, first of the shared stages: how long since the last `Tick`, and
/// the time elapsed, the sum of every delta, in nanoseconds on the steps' time (ADR-0018, ADR-0020).
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.tick")]
pub struct Tick {
    pub delta_nanos: u64,
    pub elapsed_nanos: u64,
}

impl Stage for Tick {
    const PLACE: Place = Place::Shared(0);

    fn made(time: Cycled) -> Self {
        Self { delta_nanos: time.delta_nanos, elapsed_nanos: time.elapsed_nanos }
    }
}

/// The stage in which actors draw what they hold, after `Tick`.
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.render")]
pub struct Render;

impl Stage for Render {
    const PLACE: Place = Place::Shared(1);
}

/// The stage in which what was drawn is shown, after `Render`.
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.present")]
pub struct Present;

impl Stage for Present {
    const PLACE: Place = Place::Shared(2);
}

/// One stage of a cycle, as the engine runs it: its kind, where it may stand, and how its value is written for a
/// cycle's time.
#[derive(Clone, Copy)]
pub struct Cycling {
    pub kind: KindId,
    pub place: Place,
    pub write: fn(&mut Arena, Cycled) -> Result<Payload, EncodeError>,
}

impl Cycling {
    /// The stage `S`.
    #[must_use]
    pub fn of<S: Stage>() -> Self {
        Self { kind: KindId::of::<S>(), place: S::PLACE, write: |arena, time| arena.write_mail(&S::made(time)) }
    }
}

/// The stages a chassis's cycle runs, in order: a tuple of [`Stage`]s, `()` for none (ADR-0020).
pub trait Cycle {
    /// Its stages, in order.
    fn stages() -> Vec<Cycling>;
}

/// Why a cycle was refused where the engine is made (ADR-0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unordered {
    /// It holds `Start` or `Stop`, which stand at the lifecycle's ends.
    HoldsAnEnd,
    /// It holds a stage twice.
    Twice,
    /// It holds the shared stages out of their order.
    OutOfOrder,
}

/// Checks `stages`, a cycle's in order: no end, no stage twice, and the shared ones in their order.
///
/// # Errors
///
/// What the cycle broke, as [`Unordered`] says.
pub fn ordered(stages: &[Cycling]) -> Result<(), Unordered> {
    let mut last_shared = None;
    for (at, stage) in stages.iter().enumerate() {
        let twice = stages[..at].iter().any(|before| before.kind == stage.kind);
        if twice {
            return Err(Unordered::Twice);
        }
        match stage.place {
            Place::End => return Err(Unordered::HoldsAnEnd),
            Place::Shared(rank) => {
                let is_after = last_shared.is_none_or(|last| rank > last);
                if !is_after {
                    return Err(Unordered::OutOfOrder);
                }
                last_shared = Some(rank);
            }
            Place::Own => {}
        }
    }

    Ok(())
}

impl Cycle for () {
    fn stages() -> Vec<Cycling> {
        Vec::new()
    }
}

/// A cycle of each length up to eight stages.
macro_rules! cycles {
    ($($stage:ident),+) => {
        impl<$($stage: Stage),+> Cycle for ($($stage,)+) {
            fn stages() -> Vec<Cycling> {
                alloc::vec![$(Cycling::of::<$stage>()),+]
            }
        }
    };
}

cycles!(A);
cycles!(A, B);
cycles!(A, B, C);
cycles!(A, B, C, D);
cycles!(A, B, C, D, E);
cycles!(A, B, C, D, E, F);
cycles!(A, B, C, D, E, F, G);
cycles!(A, B, C, D, E, F, G, H);
