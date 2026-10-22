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
    /// Where the stage may stand: [`End`] for `Start` and `Stop`, the stage itself for the shared stages, or [`Own`]
    /// for a chassis's own, which a cycle may hold anywhere.
    type Place: Place;

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
    type Place = End;
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
    type Place = End;
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
    type Place = Self;

    fn made(time: Cycled) -> Self {
        Self { delta_nanos: time.delta_nanos, elapsed_nanos: time.elapsed_nanos }
    }
}

/// The stage in which actors draw what they hold, after `Tick`.
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.render")]
pub struct Render;

impl Stage for Render {
    type Place = Self;
}

/// The stage in which what was drawn is shown, after `Render`.
#[derive(Plain, Default)]
#[kind(name = "aether.lifecycle.present")]
pub struct Present;

impl Stage for Present {
    type Place = Self;
}

/// One stage of a cycle, as the engine runs it: its kind, and how its value is written for a cycle's time.
#[derive(Clone, Copy)]
pub struct Cycling {
    pub kind: KindId,
    pub write: fn(&mut Arena, Cycled) -> Result<Payload, EncodeError>,
}

impl Cycling {
    /// The stage `S`.
    #[must_use]
    pub fn of<S: Stage>() -> Self {
        Self { kind: KindId::of::<S>(), write: |arena, time| arena.write_mail(&S::made(time)) }
    }
}

/// Where a stage may stand in the program's lifecycle, as its [`Stage::Place`] says (ADR-0020): [`End`], a shared
/// stage itself, or [`Own`].
pub trait Place {}

/// The place of `Start` and `Stop`, at the lifecycle's two ends, which no cycle holds.
pub struct End;

/// The place of a chassis's own stage, which a cycle may hold anywhere, before, between, or after the shared ones.
pub struct Own;

/// Where a cycle stands before it has held any shared stage: what its first shared stage follows.
pub struct Unshared;

impl Place for End {}

impl Place for Own {}

impl Place for Tick {}

impl Place for Render {}

impl Place for Present {}

/// A place a cycle may hold the stage `S` at: every place but [`End`].
#[diagnostic::on_unimplemented(
    message = "the cycle holds `{S}`, which stands at an end of the lifecycle",
    label = "a cycle that holds `{S}`",
    note = "`Start` and `Stop` are sent before and after every cycle, so no cycle holds them"
)]
pub trait InCycle<S> {}

impl<S> InCycle<S> for Own {}

impl<S> InCycle<S> for Tick {}

impl<S> InCycle<S> for Render {}

impl<S> InCycle<S> for Present {}

/// A place the stage `S` may stand at after `L`, the last shared stage the cycle held before it, [`Unshared`] for
/// none; [`Last`](Self::Last) is the last once `S` is held. A shared stage follows only those before it in their order,
/// so none follows itself; a chassis's own follows any.
#[diagnostic::on_unimplemented(
    message = "the cycle holds `{S}` after `{L}`, out of the shared stages' order",
    label = "a cycle that holds `{S}` after `{L}`",
    note = "a cycle holds the shared stages `Tick`, `Render`, and `Present` in that order, and each at most once"
)]
pub trait Follows<S, L> {
    /// The last shared stage the cycle has held, once it holds `S`.
    type Last;
}

impl<S, L> Follows<S, L> for Own {
    type Last = L;
}

/// `Start` and `Stop` follow anything, as [`InCycle`] refuses them, so a cycle that holds one is refused once.
impl<S, L> Follows<S, L> for End {
    type Last = L;
}

impl<S> Follows<S, Unshared> for Tick {
    type Last = Self;
}

impl<S> Follows<S, Unshared> for Render {
    type Last = Self;
}

impl<S> Follows<S, Tick> for Render {
    type Last = Self;
}

impl<S> Follows<S, Unshared> for Present {
    type Last = Self;
}

impl<S> Follows<S, Tick> for Present {
    type Last = Self;
}

impl<S> Follows<S, Render> for Present {
    type Last = Self;
}

/// A cycle's stages, as a list, `()` for none and `(S, R)` for the stage `S` before those of `R`, each held where it
/// may stand after `L`, the last shared stage held before them.
pub trait StandsAfter<L> {}

impl<L> StandsAfter<L> for () {}

impl<S: Stage, R, L> StandsAfter<L> for (S, R)
where
    S::Place: InCycle<S> + Follows<S, L>,
    R: StandsAfter<<S::Place as Follows<S, L>>::Last>,
{
}

/// The stages a chassis's cycle runs, in order: a tuple of [`Stage`]s, `()` for none (ADR-0020).
///
/// A tuple is a cycle only when its order is one the engine runs, which the compiler checks as the program is built:
/// it holds neither `Start` nor `Stop`, which stand at the lifecycle's ends; it holds the shared stages, `Tick`,
/// `Render`, and `Present`, in that order; and it holds no stage twice. A chassis's own stage may stand anywhere.
///
/// ```
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Tick, Render, Present)>();
/// with_cycle::<(Receive, Tick, Render)>();
/// with_cycle::<(Tick, Receive, Present)>();
/// with_cycle::<()>();
/// ```
///
/// A cycle that holds `Start` or `Stop` does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Start, Tick)>();
/// ```
///
/// Nor does one that turns the shared stages round, with or without a stage of the chassis's own between them:
///
/// ```compile_fail
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Render, Tick)>();
/// ```
///
/// ```compile_fail
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Tick, Present, Receive, Render)>();
/// ```
///
/// Nor one that holds a stage twice. A shared stage twice is out of their order, refused as `cargo check` runs; a
/// chassis's own, which no trait can tell apart from another on stable Rust, is refused by a constant, which fails
/// as the program is compiled, `cargo build` or `cargo test`, though not `cargo check`:
///
/// ```compile_fail
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Tick, Tick)>();
/// ```
///
/// ```compile_fail
/// # use aethereal_actor::{Cycle, Own, Present, Render, Stage, Start, Tick};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain, Default)]
/// # #[kind(name = "aether.example.receive")]
/// # struct Receive;
/// #
/// # impl Stage for Receive {
/// #     type Place = Own;
/// # }
/// #
/// # // What an engine asks of the cycle it is made with.
/// # fn with_cycle<C: Cycle>() {
/// #     drop(C::stages());
/// # }
/// with_cycle::<(Receive, Tick, Receive)>();
/// ```
pub trait Cycle {
    /// Its stages, in order.
    fn stages() -> Vec<Cycling>;
}

/// Whether no two of `kinds` are the same, read where a constant is made.
const fn distinct(kinds: &[KindId]) -> bool {
    let mut at = 0;
    while at < kinds.len() {
        let mut before = 0;
        while before < at {
            if kinds[before].same(kinds[at]) {
                return false;
            }
            before += 1;
        }
        at += 1;
    }

    true
}

impl Cycle for () {
    fn stages() -> Vec<Cycling> {
        Vec::new()
    }
}

/// The stages, as a list: `(A, (B, ()))` for `A, B`.
macro_rules! listed {
    () => { () };
    ($first:ident $(, $rest:ident)*) => { ($first, listed!($($rest),*)) };
}

/// A cycle of each length up to eight stages, each in its order. A shared stage held twice is refused by its order;
/// a chassis's own by a constant, which fails where the cycle's stages are compiled for an engine.
macro_rules! cycles {
    ($($stage:ident),+) => {
        impl<$($stage: Stage),+> Cycle for ($($stage,)+)
        where
            listed!($($stage),+): StandsAfter<Unshared>,
        {
            fn stages() -> Vec<Cycling> {
                const {
                    assert!(distinct(&[$(KindId::of::<$stage>()),+]), "the cycle holds a stage twice, which no cycle holds");
                }

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
