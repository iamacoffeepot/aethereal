//! How many pieces of mail a second the engine handles, and where a step's time goes.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example perf [actors...] [idle=N] [shape=NAME]`.
//! A root spawns the given number of busy actors (by default 64, 2,000, and 10,000 in turn); each sends itself a
//! count and counts it down, so every busy actor handles one piece of mail and sends one in every step. `idle=N`
//! also seats N actors that are never sent anything, and `shape=NAME` runs one shape of work only. Each line is the
//! best of three runs: mails a second, nanoseconds a piece of mail, and a step's time split into planning, running,
//! and applying.
//!
//! Each run is a program of one root, the counter that spawns the rest, bootstrapped on the headless host with its
//! settings and the engine's set in code (ADR-0029).

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Recursive, ToType, Turn};
use aethereal_chassis_headless::{Headless, Measured};
use aethereal_data::Plain;
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use core::num::NonZeroU32;
use std::env;
use std::error::Error;
use std::hint;

/// How many turns a counter has left.
#[derive(Plain)]
#[kind(name = "aether.bench.count")]
struct Count {
    left: u64,
}

/// What a counter is made from: its number, and for the root how many counters to spawn and of what cost, and how
/// many idle ones beside them.
#[derive(Plain)]
#[kind(name = "aether.bench.counter_config")]
struct CounterConfig {
    number: u64,
    children: u64,
    idle: u64,
    shape: u8,
}

/// The counters a run spawns: how many busy and how many idle, and the shape of the busy ones' work.
#[derive(Plain, Default)]
#[kind(name = "aether.bench.counters")]
#[settings]
struct Counters {
    children: u64,
    idle: u64,
    shape: u8,
}

/// An actor that counts itself down, working as its shape says on each count.
#[derive(Plain)]
#[kind(name = "aether.bench.counter")]
struct Counter {
    number: u64,
    shape: u8,
}

/// The quota for the bytes of mail in flight, shared among the workers.
const MAIL_BYTES: u64 = 1 << 30;

/// The most bytes the engine's blobs hold.
const BLOB_BYTES: u64 = 1 << 30;

/// The parts the engine's views are kept in.
const PARTS: NonZeroU32 = NonZeroU32::new(16).expect("16 is not zero");

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

/// The root counter stands up from the counters' settings, on any host.
impl<H: Host> Bootstrap<H> for Counter {
    const SECTION: &'static str = "counters";
    type Settings = Counters;
    type Needs = ();
    type Provides = ();

    fn bootstrap(counters: Counters, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let Counters { children, idle, shape } = counters;
        cx.begin(&0, CounterConfig { number: 0, children, idle, shape })
    }
}

/// One run of `children` counters of `shape`, beside `idle` idle ones, to idle on `workers` workers: the nanoseconds
/// it took, and what its steps cost.
fn run(children: u64, idle: u64, shape: u8, workers: NonZeroU32) -> Result<(u64, Measured), Box<dyn Error>> {
    let mut headless = Headless::program()
        .root::<Counter>()
        .set_engine(EngineSettings::WORKERS.to(workers))
        .set_engine(EngineSettings::PARTS.to(PARTS))
        .set_engine(EngineSettings::MAIL_BYTES.to(MAIL_BYTES))
        .set_engine(EngineSettings::BLOB_BYTES.to(BLOB_BYTES))
        .set::<Counter>(Counters::CHILDREN.to(children))
        .set::<Counter>(Counters::IDLE.to(idle))
        .set::<Counter>(Counters::SHAPE.to(shape))
        .build()?;
    headless.run_until_idle();

    let measured = headless.measured();
    Ok((measured.plan_nanos + measured.step_nanos + measured.apply_nanos, measured))
}

fn main() -> Result<(), Box<dyn Error>> {
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
            for workers in [1_u32, 4, 16] {
                let workers = NonZeroU32::try_from(workers)?;
                let runs = (0..3).map(|_| run(children, idle, shape, workers)).collect::<Result<Vec<_>, _>>()?;
                let (took, measured) = runs.into_iter().min_by_key(|(took, _)| *took).unwrap_or_default();
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

    Ok(())
}
