//! Four programs of the Savina actor benchmark suite, to set beside the same programs on other actor frameworks.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example savina [program...] [workers=N,...]`.
//! The programs, at Savina's default sizes:
//!
//! - `pingpong`: two actors send one piece of mail back and forth 40,000 times each way, so only one piece of mail
//!   is in flight at a time;
//! - `count`: one actor sends another 1,000,000 pieces of mail, which it counts, then asks for the count;
//! - `fjthrput`: one actor sends each of 60 others 10,000 pieces of mail, each of which they do a little sum for;
//! - `big`: 120 actors each ping a neighbour picked at random 20,000 times, the neighbour answering each ping.
//!
//! As in Savina, each program's root sends its first mail from a turn, on a start message: it tells its last child
//! the program can start as it begins, and sends on the child's answer. `count-begin` is `count` with the producer sending everything from its `begin`
//! instead, which runs between steps: it is beside the programs to show what a root that sends a great deal as it
//! begins costs, and has no counterpart on the other frameworks.
//!
//! Each line is the best of three runs on the given numbers of workers (by default 1, 4, and 16), timed from the
//! root's `begin` to the last step, which takes in starting the workers: the pieces of mail sent, mails a second,
//! nanoseconds a piece of mail, and the steps the run took.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

use aethereal_actor::{Actor, Beginning, Ending, ErasedActorRef, Fault, Item, To, ToWithoutMail, Turn};
use aethereal_chassis_headless::{Headless, Monotonic};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::{Clock, Engine};
use std::env;
use std::hint;
use std::num::NonZeroUsize;

/// How many times each way the ping-pong mail goes.
const PINGS: u64 = 40_000;
/// How many pieces of mail the counter counts.
const COUNTS: u64 = 1_000_000;
/// How many actors take mail in the fork-join program, and how many pieces each.
const FORKS: u64 = 60;
const FORKED: u64 = 10_000;
/// How many actors ping each other in the big program, and how many pings each sends.
const BIG_ACTORS: u64 = 120;
const BIG_PINGS: u64 = 20_000;

/// A ping with this many to go after it.
#[derive(Plain)]
struct Ping {
    left: u64,
}

impl Kind for Ping {
    const NAME: KindName = KindName::new("aether.savina.ping");
}

/// The answer to a ping.
#[derive(Plain)]
struct Pong {
    left: u64,
}

impl Kind for Pong {
    const NAME: KindName = KindName::new("aether.savina.pong");
}

/// One to count.
#[derive(Plain)]
struct Increment;

impl Kind for Increment {
    const NAME: KindName = KindName::new("aether.savina.increment");
}

/// A request for the count so far.
#[derive(Plain)]
struct Retrieve;

impl Kind for Retrieve {
    const NAME: KindName = KindName::new("aether.savina.retrieve");
}

/// The count, answering a [`Retrieve`].
#[derive(Plain)]
struct Total {
    count: u64,
}

impl Kind for Total {
    const NAME: KindName = KindName::new("aether.savina.total");
}

/// A piece of work in the fork-join program.
#[derive(Plain)]
struct Work;

impl Kind for Work {
    const NAME: KindName = KindName::new("aether.savina.work");
}

/// A neighbour, handed to an actor of the big program.
#[derive(Bound)]
struct Neighbour {
    other: ErasedActorRef,
}

impl Kind for Neighbour {
    const NAME: KindName = KindName::new("aether.savina.neighbour");
}

/// The word to start pinging.
#[derive(Plain)]
struct Start;

impl Kind for Start {
    const NAME: KindName = KindName::new("aether.savina.start");
}

/// The word that a program can start. A root sends it to its last child as it begins, the child answers it from a
/// turn, and the root sends its first mail on that answer: from an ordinary turn, as Savina's producer acts on a
/// start message, and not from its `begin`.
#[derive(Plain)]
struct Ready;

impl Kind for Ready {
    const NAME: KindName = KindName::new("aether.savina.ready");
}

/// What every actor here is made from: its number.
#[derive(Plain)]
struct Number {
    number: u64,
}

impl Kind for Number {
    const NAME: KindName = KindName::new("aether.savina.number");
}

/// The ping side of the ping-pong program, and its root: it spawns the pong side and serves first.
#[derive(Plain)]
struct Pinger;

impl Kind for Pinger {
    const NAME: KindName = KindName::new("aether.savina.pinger");
}

impl Actor for Pinger {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let Ok(ponger) = turn.spawn::<Ponger>(&0, Number { number: 0 }) else {
            panic!("the pong side begins");
        };
        turn.send(ToWithoutMail::Held(ponger), &Ready)?;

        Ok(Self)
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Ready)) {
            turn.send(To::Sender, &Ping { left: PINGS - 1 })?;
        } else if let Some(Pong { left }) = mail.read()?
            && left > 0
        {
            turn.send(To::Sender, &Ping { left: left - 1 })?;
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The pong side of the ping-pong program: it answers each ping.
#[derive(Plain)]
struct Ponger;

impl Kind for Ponger {
    const NAME: KindName = KindName::new("aether.savina.ponger");
}

impl Actor for Ponger {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, _: &mut Beginning<'_>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if let Some(Ping { left }) = mail.read()? {
            turn.send(To::Sender, &Pong { left })?;
        } else if matches!(mail.read()?, Some(Ready)) {
            turn.send(To::Sender, &Ready)?;
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The producer of the counting program, and its root: it spawns the counter, and once the counter is ready sends
/// it everything to count, then asks for the count.
#[derive(Plain)]
struct Producer;

impl Kind for Producer {
    const NAME: KindName = KindName::new("aether.savina.producer");
}

impl Actor for Producer {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn::<Counter>(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        turn.send(ToWithoutMail::Held(counter), &Ready)?;

        Ok(Self)
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Ready)) {
            for _ in 0..COUNTS {
                turn.send(To::Sender, &Increment)?;
            }
            turn.send(To::Sender, &Retrieve)?;
        } else if let Some(Total { count }) = mail.read()? {
            assert_eq!(count, COUNTS, "the counter counts every piece of mail");
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The counting program with its producer sending everything from its `begin`, between steps: what a root that
/// sends a great deal as it begins costs.
#[derive(Plain)]
struct BeginProducer;

impl Kind for BeginProducer {
    const NAME: KindName = KindName::new("aether.savina.begin_producer");
}

impl Actor for BeginProducer {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn::<Counter>(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        for _ in 0..COUNTS {
            turn.send(ToWithoutMail::Held(counter), &Increment)?;
        }
        turn.send(ToWithoutMail::Held(counter), &Retrieve)?;

        Ok(Self)
    }

    fn handle(&mut self, item: Item<'_>, _: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if let Some(Total { count }) = mail.read()? {
            assert_eq!(count, COUNTS, "the counter counts every piece of mail");
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The counter of the counting program.
#[derive(Plain)]
struct Counter {
    count: u64,
}

impl Kind for Counter {
    const NAME: KindName = KindName::new("aether.savina.counter");
}

impl Actor for Counter {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, _: &mut Beginning<'_>) -> Result<Self, Fault> {
        Ok(Self { count: 0 })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Increment)) {
            self.count += 1;
        } else if matches!(mail.read()?, Some(Retrieve)) {
            turn.send(To::Sender, &Total { count: self.count })?;
        } else if matches!(mail.read()?, Some(Ready)) {
            turn.send(To::Sender, &Ready)?;
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The root of the fork-join program: it spawns the workers, and once they are ready sends each its share of the
/// work.
#[derive(Bound)]
struct Forker {
    workers: Vec<ErasedActorRef>,
}

impl Kind for Forker {
    const NAME: KindName = KindName::new("aether.savina.forker");
}

impl Actor for Forker {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let workers: Vec<ErasedActorRef> = (0..FORKS).map(|number| spawned::<Forked>(turn, number)).collect();
        ready(turn, &workers)?;

        Ok(Self { workers })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Ready)) {
            for _ in 0..FORKED {
                for worker in &self.workers {
                    turn.send(To::Held(*worker), &Work)?;
                }
            }
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A worker of the fork-join program: it does Savina's little sum for each piece of work.
#[derive(Plain)]
struct Forked;

impl Kind for Forked {
    const NAME: KindName = KindName::new("aether.savina.forked");
}

impl Actor for Forked {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, _: &mut Beginning<'_>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Work)) {
            let sine = hint::black_box(37.2_f64).sin();
            hint::black_box(sine * sine);
        } else if matches!(mail.read()?, Some(Ready)) {
            turn.send(To::Sender, &Ready)?;
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The root of the big program: it spawns the actors, and once they are ready introduces each to every other and
/// starts them.
#[derive(Bound)]
struct Crowd {
    actors: Vec<ErasedActorRef>,
}

impl Kind for Crowd {
    const NAME: KindName = KindName::new("aether.savina.crowd");
}

impl Actor for Crowd {
    type Config = Number;
    type Key = u64;

    fn begin(_: Number, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let actors: Vec<ErasedActorRef> = (0..BIG_ACTORS).map(|number| spawned::<Pinging>(turn, number)).collect();
        ready(turn, &actors)?;

        Ok(Self { actors })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if matches!(mail.read()?, Some(Ready)) {
            for actor in &self.actors {
                for other in self.actors.iter().filter(|other| *other != actor) {
                    turn.send(To::Held(*actor), &Neighbour { other: *other })?;
                }
                turn.send(To::Held(*actor), &Start)?;
            }
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor of the big program: it pings a neighbour picked at random, and on each answer pings again until it has
/// sent all its pings. It answers every ping it is sent.
#[derive(Bound)]
struct Pinging {
    neighbours: Vec<ErasedActorRef>,
    /// The state of its random numbers.
    random: u64,
}

impl Kind for Pinging {
    const NAME: KindName = KindName::new("aether.savina.pinging");
}

impl Pinging {
    /// Pings a neighbour picked at random, with `left` to go after it.
    fn ping(&mut self, turn: &mut Turn<'_>, left: u64) -> Result<(), Fault> {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        let count = u64::try_from(self.neighbours.len()).unwrap_or(1).max(1);
        let picked = usize::try_from(self.random % count).unwrap_or(0);
        turn.send(To::Held(self.neighbours[picked]), &Ping { left })?;

        Ok(())
    }
}

impl Actor for Pinging {
    type Config = Number;
    type Key = u64;

    fn begin(config: Number, _: &mut Beginning<'_>) -> Result<Self, Fault> {
        Ok(Self { neighbours: Vec::new(), random: config.number * 0x9e37_79b9_7f4a_7c15 + 1 })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        if let Some(Neighbour { other }) = mail.read()? {
            self.neighbours.push(other);
        } else if matches!(mail.read()?, Some(Start)) {
            self.ping(turn, BIG_PINGS - 1)?;
        } else if let Some(Ping { left }) = mail.read()? {
            turn.send(To::Sender, &Pong { left })?;
        } else if let Some(Pong { left }) = mail.read()?
            && left > 0
        {
            self.ping(turn, left - 1)?;
        } else if matches!(mail.read()?, Some(Ready)) {
            turn.send(To::Sender, &Ready)?;
        }

        Ok(())
    }

    fn end(self, _: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Tells the last of `children` that the program can start, so it answers from a turn.
fn ready(turn: &mut Beginning<'_>, children: &[ErasedActorRef]) -> Result<(), Fault> {
    if let Some(last) = children.last() {
        turn.send(ToWithoutMail::Held(*last), &Ready)?;
    }

    Ok(())
}

/// A child of type `A` numbered `number`, spawned by `turn`.
fn spawned<A: Actor<Config = Number, Key = u64> + 'static>(turn: &mut Beginning<'_>, number: u64) -> ErasedActorRef {
    let Ok(child) = turn.spawn::<A>(&number, Number { number }) else {
        panic!("a child begins");
    };

    child
}

/// One program: its name, how many actors it runs, how many pieces of mail it sends, and how to start it.
struct Program {
    name: &'static str,
    actors: u64,
    mails: u64,
    start: fn(&mut Engine),
}

/// Adds the root of type `A`, which starts a program.
fn root<A: Actor<Config = Number, Key = u64> + 'static>(engine: &mut Engine) {
    let Ok(_) = engine.add_root::<A>(&0, Number { number: 0 }) else {
        panic!("the root begins");
    };
}

const PROGRAMS: [Program; 5] = [
    Program { name: "pingpong", actors: 2, mails: 2 * PINGS, start: root::<Pinger> },
    Program { name: "count", actors: 2, mails: COUNTS + 2, start: root::<Producer> },
    Program { name: "count-begin", actors: 2, mails: COUNTS + 2, start: root::<BeginProducer> },
    Program { name: "fjthrput", actors: FORKS + 1, mails: FORKS * FORKED, start: root::<Forker> },
    Program {
        name: "big",
        actors: BIG_ACTORS + 1,
        mails: BIG_ACTORS * (BIG_ACTORS - 1) + BIG_ACTORS + 2 * BIG_ACTORS * BIG_PINGS,
        start: root::<Crowd>,
    },
];

/// One run of `program` on `workers` workers: the nanoseconds it took, from its root's `begin` to the last step,
/// and the steps.
fn run(program: &Program, workers: NonZeroUsize) -> (u64, u64) {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let mut engine = Engine::new(parts, 1024);
    let clock = Monotonic::new();
    (program.start)(&mut engine);
    let mut headless = Headless::new(engine, workers);
    headless.run_until_idle();

    (clock.nanos(), headless.measured().steps)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let named: Vec<&str> = args.iter().filter(|arg| !arg.contains('=')).map(String::as_str).collect();
    let workers: Vec<usize> = args
        .iter()
        .find_map(|arg| arg.strip_prefix("workers="))
        .map_or_else(|| vec![1, 4, 16], |given| given.split(',').filter_map(|count| count.parse().ok()).collect());

    println!("program     actors workers      mails   M mails/s  ns/mail    steps");
    for program in &PROGRAMS {
        let is_skipped = !named.is_empty() && !named.contains(&program.name);
        if is_skipped {
            continue;
        }
        for &count in &workers {
            let workers = NonZeroUsize::MIN.saturating_add(count.max(1) - 1);
            let runs = (0..3).map(|_| run(program, workers));
            let (took, steps) = runs.min_by_key(|(took, _)| *took).unwrap_or_default();
            println!(
                "{:11} {:6} {workers:7} {:10}   {:9.2} {:8} {steps:8}",
                program.name,
                program.actors,
                program.mails,
                program.mails as f64 * 1e3 / took.max(1) as f64,
                took / program.mails.max(1),
            );
        }
    }
}
