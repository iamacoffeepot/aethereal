//! How many pieces of mail a second the engine handles, and where a step's time goes.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example perf [actors...]`. A root spawns the
//! given number of actors (by default 64, 2,000, and 10,000 in turn); each sends itself a count and counts it
//! down, so every actor handles one piece of mail and sends one in every step. Each line is the best of three
//! runs: mails a second, nanoseconds a piece of mail, and a step's time split into planning, running, and
//! applying.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

use aethereal_actor::{Actor, Beginning, Ending, Fault, Item, To, ToWithoutMail, Turn};
use aethereal_chassis_headless::{Headless, Measured};
use aethereal_data::{Kind, KindName, Plain, TypedKey};
use aethereal_substrate::Engine;
use std::env;
use std::hint;
use std::num::NonZeroUsize;

/// How many turns a counter has left.
#[derive(Plain)]
struct Count {
    left: u64,
}

impl Kind for Count {
    const NAME: KindName = KindName::new("aether.bench.count");
}

/// What a counter is made from: its number, and for the root how many counters to spawn and of what cost.
#[derive(Plain)]
struct CounterConfig {
    number: u64,
    children: u64,
    shape: u8,
}

impl Kind for CounterConfig {
    const NAME: KindName = KindName::new("aether.bench.counter_config");
}

/// An actor that counts itself down, working as its shape says on each count.
#[derive(Plain)]
struct Counter {
    number: u64,
    shape: u8,
}

impl Kind for Counter {
    const NAME: KindName = KindName::new("aether.bench.counter");
}

/// How many counts each counter takes.
const TURNS: u64 = 50;

/// The shapes of work a turn does.
const SHAPES: [(u8, &str); 3] = [(0, "trivial"), (1, "uneven"), (2, "tail")];

impl Counter {
    /// The units of work in this counter's turn with `left` counts to go.
    const fn units(&self, left: u64) -> u64 {
        match self.shape {
            0 => 0,
            1 => 25 + (self.number % 32) * 25,
            _ => {
                let is_heavy = mix(self.number * 1_000 + left).is_multiple_of(20);
                if is_heavy {
                    2_000
                } else {
                    20
                }
            }
        }
    }
}

/// A well-spread hash of `value`.
const fn mix(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);

    value ^ (value >> 33)
}

impl Actor for Counter {
    type Config = CounterConfig;
    type Key = u64;

    fn begin(config: CounterConfig, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        for number in 1..=config.children {
            let child = CounterConfig { number, children: 0, shape: config.shape };
            if turn.spawn::<Self>(&number, child).is_ok() {
                turn.send(ToWithoutMail::ChildInstanced(Self::NAME, number.to_name()), &Count { left: TURNS })?;
            }
        }

        Ok(Self { number: config.number, shape: config.shape })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        let Some(Count { left }) = mail.read()? else {
            return Ok(());
        };

        let worked = (0..self.units(left) * 10).fold(self.number, |sum, step| sum.wrapping_mul(31).wrapping_add(step));
        hint::black_box(worked);
        if left == 0 {
            turn.close();
        } else {
            turn.send(To::Held(turn.me()), &Count { left: left - 1 })?;
        }

        Ok(())
    }

    fn end(self, _turn: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// One run of `children` counters of `shape` to idle on `workers` workers: the nanoseconds it took, and what its
/// steps cost.
fn run(children: u64, shape: u8, workers: NonZeroUsize) -> (u64, Measured) {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let mut engine = Engine::new(parts, 1024);
    let Ok(_) = engine.add_root::<Counter>(&0, CounterConfig { number: 0, children, shape }) else {
        panic!("the root begins");
    };
    let mut headless = Headless::new(engine, workers);
    headless.run_until_idle();

    let measured = headless.measured();
    (measured.plan_nanos + measured.step_nanos + measured.apply_nanos, measured)
}

fn main() {
    let given: Vec<u64> = env::args().skip(1).filter_map(|actors| actors.parse().ok()).collect();
    let counts = if given.is_empty() {
        vec![64, 2_000, 10_000]
    } else {
        given
    };

    println!("shape    actors workers   M mails/s  ns/mail   plan µs/step   run µs/step   apply µs/step");
    for (shape, name) in SHAPES {
        for &children in &counts {
            for workers in [1, 4, 16] {
                let workers = NonZeroUsize::MIN.saturating_add(workers - 1);
                let (took, measured) =
                    (0..3).map(|_| run(children, shape, workers)).min_by_key(|(took, _)| *took).unwrap_or_default();
                let mails = children * (TURNS + 1);
                let steps = measured.steps.max(1);
                println!(
                    "{name:8} {children:6} {workers:7}   {:9.2} {:8}   {:12}   {:11}   {:13}",
                    mails as f64 * 1e3 / took.max(1) as f64,
                    took / mails.max(1),
                    measured.plan_nanos / steps / 1_000,
                    measured.step_nanos / steps / 1_000,
                    measured.apply_nanos / steps / 1_000,
                );
            }
        }
    }
}
