//! How many pieces of mail a second the engine handles, and where a step's time goes.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example perf [actors...] [idle=N] [shape=NAME]`.
//! A root spawns the given number of busy actors (by default 64, 2,000, and 10,000 in turn); each sends itself a
//! count and counts it down, so every busy actor handles one piece of mail and sends one in every step. `idle=N`
//! also seats N actors that are never sent anything, and `shape=NAME` runs one shape of work only. Each line is the
//! best of three runs: mails a second, nanoseconds a piece of mail, and a step's time split into planning, running,
//! and applying.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Recursive, ToType, Turn};
use aethereal_chassis_headless::{Headless, Measured};
use aethereal_data::{Kind, KindName, Plain};
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

/// What a counter is made from: its number, and for the root how many counters to spawn and of what cost, and how
/// many idle ones beside them.
#[derive(Plain)]
struct CounterConfig {
    number: u64,
    children: u64,
    idle: u64,
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

/// The quota for the bytes of mail in flight, shared among the workers.
const MAIL_BYTES: usize = 1 << 30;

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

impl ChildOf for Counter {
    type Parent = Recursive;
}

impl Actor for Counter {
    type Config = CounterConfig;
    type Key = u64;
    type Handles = (Count,);
    type BeginSends = (ToType<Self, Count>,);
    type EndSends = ();

    fn begin(config: CounterConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for number in 1..=config.children + config.idle {
            let child = CounterConfig { number, children: 0, idle: 0, shape: config.shape };
            let is_busy = number <= config.children;
            let spawned = turn.spawn::<Self>(&number, child);
            if let Ok(child) = spawned
                && is_busy
            {
                turn.to(child).send(&Count { left: TURNS })?;
            }
        }

        Ok(Self { number: config.number, shape: config.shape })
    }

    fn end(self, _turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Count> for Counter {
    type Sends = (ToType<Self, Count>,);

    fn handle(&mut self, Count { left }: Count, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let worked = (0..self.units(left) * 10).fold(self.number, |sum, step| sum.wrapping_mul(31).wrapping_add(step));
        hint::black_box(worked);
        if left == 0 {
            turn.close();
        } else {
            turn.to(turn.me()).send(&Count { left: left - 1 })?;
        }

        Ok(())
    }
}

/// One run of `children` counters of `shape`, beside `idle` idle ones, to idle on `workers` workers: the nanoseconds
/// it took, and what its steps cost.
fn run(children: u64, idle: u64, shape: u8, workers: NonZeroUsize) -> (u64, Measured) {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let mut engine = Engine::new(parts, 1024, MAIL_BYTES, 1 << 30, 0).ready();
    let Ok(_) = engine.add_root::<Counter>(&0, CounterConfig { number: 0, children, idle, shape }) else {
        panic!("the root begins");
    };
    let mut headless = Headless::new(engine, workers);
    headless.run_until_idle();

    let measured = headless.measured();
    (measured.plan_nanos + measured.step_nanos + measured.apply_nanos, measured)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let given: Vec<u64> = args.iter().filter_map(|actors| actors.parse().ok()).collect();
    let counts = if given.is_empty() {
        vec![64, 2_000, 10_000]
    } else {
        given
    };
    let idle = args.iter().find_map(|arg| arg.strip_prefix("idle=")?.parse().ok()).unwrap_or(0);
    let only = args.iter().find_map(|arg| arg.strip_prefix("shape="));

    println!("shape    actors    idle workers   M mails/s  ns/mail   plan µs/step   run µs/step   apply µs/step");
    for (shape, name) in SHAPES {
        let is_skipped = only.is_some_and(|only| only != name);
        if is_skipped {
            continue;
        }
        for &children in &counts {
            for workers in [1, 4, 16] {
                let workers = NonZeroUsize::MIN.saturating_add(workers - 1);
                let runs = (0..3).map(|_| run(children, idle, shape, workers));
                let (took, measured) = runs.min_by_key(|(took, _)| *took).unwrap_or_default();
                let mails = children * (TURNS + 1);
                let steps = measured.steps.max(1);
                println!(
                    "{name:8} {children:6} {idle:7} {workers:7}   {:9.2} {:8}   {:12}   {:11}   {:13}",
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
