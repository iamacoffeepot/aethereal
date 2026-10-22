//! Six programs of the Savina actor benchmark suite, to set beside the same programs on other actor frameworks, and
//! two of this engine's own.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example savina [program...] [workers=N,...]
//! [hops=N] [rounds=N]`, where `hops` is how many fused hops each part may run in a step (ADR-0013), none by default,
//! and `rounds`, when given, runs each program that many times one after another on one engine, as a long-running
//! engine runs, and reports it cold, then steady once the engine has learned: each round's cost of a piece of mail is
//! its whole run's, and the steady figures are the median and the best of the rounds after the first third.
//! The programs, at Savina's default sizes:
//!
//! - `pingpong`: two actors send one piece of mail back and forth 40,000 times each way, so only one piece of mail
//!   is in flight at a time;
//! - `count`: one actor sends another 1,000,000 pieces of mail, which it counts, then asks for the count;
//! - `fjthrput`: one actor sends each of 60 others 10,000 pieces of mail, each of which they do a little sum for;
//! - `big`: 120 actors each ping a neighbour picked at random 20,000 times, the neighbour answering each ping;
//! - `fib`: the 25th Fibonacci number, each actor asked for one spawning two children for the two before it, which
//!   answer and close, so about 150,000 actors are made and closed;
//! - `sieve`: the primes below 100,000, odd numbers passed down a pipeline of filters that each keep up to 1,000
//!   primes, a filter spawning the next when it is full.
//!
//! And three of this engine's own, with no counterpart on the other frameworks:
//!
//! - `churn`: 1,000 actors alive at a time, each living 8 hops of mail to itself, then telling its parent and
//!   closing, and the parent spawning one in its place, until 100,000 have lived: spawning and closing at a steady
//!   population, as a game makes and ends what lives in it.
//! - `churn-wide`: `churn` split among 16 roots, each keeping a sixteenth of the population alive and spawning a
//!   sixteenth of the total: the same work with no one actor that every spawn goes through, as a game whose many
//!   places each make and end their own.
//! - `world`: 16 regions of 256 bodies each, run 64 frames in lockstep, as a game's world runs: each frame a region
//!   sends each body its frame, the body moves, nudges the bodies either side of it, and answers, and the region
//!   starts the next frame once all have answered. A body lives up to 32 frames, a few expiring each frame, and the
//!   region puts a new one in the place of each.
//!
//! As in Savina, each program's root sends its first mail from a turn, on a start message: it tells its last child
//! the program can start as it begins, and sends on the child's answer. `count-begin` is `count` with the producer sending everything from its `begin`
//! instead, which runs between steps: it is beside the programs to show what a root that sends a great deal as it
//! begins costs, and has no counterpart on the other frameworks.
//!
//! Each line is the best of three runs on the given numbers of workers (by default 1, 4, and 16), timed from the
//! root's `begin` to the last step, which takes in starting the workers: the pieces of mail sent, mails a second,
//! nanoseconds a piece of mail, and the steps the run took. With hops, a line under it counts the sends made while a
//! part could still fuse: those fused, and each other by the rule that sent it out as mail.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

use aethereal_actor::{
    Actor, ActorRef, Admits, Allows, Anywhere, Beginning, ChildOf, Ending, Fault, Handles, Root, ToAny, ToSender,
    ToType, Top, Turn, parent,
};
use aethereal_chassis_headless::{Headless, Monotonic};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Clock, Engine, Hops};
use std::env;
use std::hint;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};

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
/// Which Fibonacci number the fib program works out.
const FIB: u64 = 25;
/// The sieve program's limit, and how many primes each of its filters keeps.
const SIEVE_LIMIT: u64 = 100_000;
const SIEVE_KEPT: usize = 1_000;
/// How many actors the churn program keeps alive, how many hops each lives, and how many live in all.
const CHURN_ALIVE: u64 = 1_000;
const CHURN_LIFE: u64 = 8;
const CHURN_TOTAL: u64 = 100_000;
/// How many roots the wide churn program splits the churn program among.
const CHURN_SPAWNERS: u64 = 16;
/// How many regions the world program has, how many bodies each keeps alive, how many frames it runs, and how many
/// frames a body lives.
const WORLD_REGIONS: u64 = 16;
const WORLD_BODIES: u64 = 256;
const WORLD_FRAMES: u64 = 64;
const WORLD_LIFE: u64 = 32;

/// A ping with this many to go after it.
#[derive(Plain)]
#[kind(name = "aether.savina.ping")]
struct Ping {
    left: u64,
}

/// The answer to a ping.
#[derive(Plain)]
#[kind(name = "aether.savina.pong")]
struct Pong {
    left: u64,
}

/// One to count.
#[derive(Plain)]
#[kind(name = "aether.savina.increment")]
struct Increment;

/// A request for the count so far.
#[derive(Plain)]
#[kind(name = "aether.savina.retrieve")]
struct Retrieve;

/// The count, answering a [`Retrieve`].
#[derive(Plain)]
#[kind(name = "aether.savina.total")]
struct Total {
    count: u64,
}

/// A piece of work in the fork-join program.
#[derive(Plain)]
#[kind(name = "aether.savina.work")]
struct Work;

/// A neighbour, handed to an actor of the big program.
#[derive(Bound)]
#[kind(name = "aether.savina.neighbour")]
struct Neighbour {
    other: ActorRef<Pinging>,
}

/// The word to start pinging.
#[derive(Plain)]
#[kind(name = "aether.savina.start")]
struct Start;

/// The word that a program can start. A root sends it to its last child as it begins, the child answers it from a
/// turn, and the root sends its first mail on that answer: from an ordinary turn, as Savina's producer acts on a
/// start message, and not from its `begin`.
#[derive(Plain)]
#[kind(name = "aether.savina.ready")]
struct Ready;

/// What every actor here is made from: its number.
#[derive(Plain)]
#[kind(name = "aether.savina.number")]
struct Number {
    number: u64,
}

/// The ping side of the ping-pong program, and its root: it spawns the pong side and serves first.
#[derive(Plain)]
#[kind(name = "aether.savina.pinger")]
struct Pinger;

impl ChildOf for Pinger {
    type Parent = Top;
}

impl Actor for Pinger {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready, Pong);
    type BeginSends = (ToType<Ponger, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(ponger) = turn.spawn(&0, Number { number: 0 }) else {
            panic!("the pong side begins");
        };
        turn.to(ponger).send(&Ready)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Pinger {
    type Sends = (ToSender<Ping>,);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Ping { left: PINGS - 1 })?;

        Ok(())
    }
}

impl Handles<Pong> for Pinger {
    type Sends = (ToSender<Ping>,);

    fn handle(&mut self, Pong { left }: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if left > 0 {
            turn.reply(&Ping { left: left - 1 })?;
        }

        Ok(())
    }
}

/// The pong side of the ping-pong program: it answers each ping.
#[derive(Plain)]
#[kind(name = "aether.savina.ponger")]
struct Ponger;

impl ChildOf for Ponger {
    type Parent = Pinger;
}

impl Actor for Ponger {
    type Config = Number;
    type Key = u64;
    type Handles = (Ping, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Ponger {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { left })?;

        Ok(())
    }
}

/// The producer of the counting program, and its root: it spawns the counter, and once the counter is ready sends
/// it everything to count, then asks for the count.
#[derive(Plain)]
#[kind(name = "aether.savina.producer")]
struct Producer;

impl ChildOf for Producer {
    type Parent = Top;
}

impl Actor for Producer {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready, Total);
    type BeginSends = (ToType<Counter, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        turn.to(counter).send(&Ready)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Producer {
    type Sends = (ToSender<Increment>, ToSender<Retrieve>);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for _ in 0..COUNTS {
            turn.reply(&Increment)?;
        }
        turn.reply(&Retrieve)?;

        Ok(())
    }
}

impl Handles<Total> for Producer {
    type Sends = ();

    fn handle(&mut self, Total { count }: Total, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        assert_eq!(count, COUNTS, "the counter counts every piece of mail");

        Ok(())
    }
}

/// The counting program with its producer sending everything from its `begin`, between steps: what a root that
/// sends a great deal as it begins costs.
#[derive(Plain)]
#[kind(name = "aether.savina.begin_producer")]
struct BeginProducer;

impl ChildOf for BeginProducer {
    type Parent = Top;
}

impl Actor for BeginProducer {
    type Config = Number;
    type Key = u64;
    type Handles = (Total,);
    type BeginSends = (ToType<Counter, Increment>, ToType<Counter, Retrieve>);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        for _ in 0..COUNTS {
            turn.to(counter).send(&Increment)?;
        }
        turn.to(counter).send(&Retrieve)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Total> for BeginProducer {
    type Sends = ();

    fn handle(&mut self, Total { count }: Total, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        assert_eq!(count, COUNTS, "the counter counts every piece of mail");

        Ok(())
    }
}

/// The counter of the counting program.
#[derive(Plain)]
#[kind(name = "aether.savina.counter")]
struct Counter {
    count: u64,
}

impl ChildOf for Counter {
    type Parent = Anywhere;
}

impl Actor for Counter {
    type Config = Number;
    type Key = u64;
    type Handles = (Increment, Retrieve, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { count: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Increment> for Counter {
    type Sends = ();

    fn handle(&mut self, _: Increment, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.count += 1;

        Ok(())
    }
}

impl Handles<Retrieve> for Counter {
    type Sends = (ToSender<Total>,);

    fn handle(&mut self, _: Retrieve, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Total { count: self.count })?;

        Ok(())
    }
}

/// The root of the fork-join program: it spawns the workers, and once they are ready sends each its share of the
/// work.
#[derive(Bound)]
#[kind(name = "aether.savina.forker")]
struct Forker {
    workers: Vec<ActorRef<Forked>>,
}

impl ChildOf for Forker {
    type Parent = Top;
}

impl Actor for Forker {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready,);
    type BeginSends = (ToType<Forked, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let workers: Vec<ActorRef<Forked>> = (0..FORKS).map(|number| spawned(turn, number)).collect();
        ready(turn, &workers)?;

        Ok(Self { workers })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Forker {
    type Sends = (ToType<Forked, Work>,);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for _ in 0..FORKED {
            for worker in &self.workers {
                turn.to(*worker).send(&Work)?;
            }
        }

        Ok(())
    }
}

/// A worker of the fork-join program: it does Savina's little sum for each piece of work.
#[derive(Plain)]
#[kind(name = "aether.savina.forked")]
struct Forked;

impl ChildOf for Forked {
    type Parent = Forker;
}

impl Actor for Forked {
    type Config = Number;
    type Key = u64;
    type Handles = (Work, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Work> for Forked {
    type Sends = ();

    fn handle(&mut self, _: Work, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sine = hint::black_box(37.2_f64).sin();
        hint::black_box(sine * sine);

        Ok(())
    }
}

/// The root of the big program: it spawns the actors, and once they are ready introduces each to every other and
/// starts them.
#[derive(Bound)]
#[kind(name = "aether.savina.crowd")]
struct Crowd {
    actors: Vec<ActorRef<Pinging>>,
}

impl ChildOf for Crowd {
    type Parent = Top;
}

impl Actor for Crowd {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready,);
    type BeginSends = (ToType<Pinging, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let actors: Vec<ActorRef<Pinging>> = (0..BIG_ACTORS).map(|number| spawned(turn, number)).collect();
        ready(turn, &actors)?;

        Ok(Self { actors })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Crowd {
    type Sends = (ToType<Pinging, Neighbour>, ToType<Pinging, Start>);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for actor in &self.actors {
            for other in self.actors.iter().filter(|other| *other != actor) {
                turn.to(*actor).send(&Neighbour { other: *other })?;
            }
            turn.to(*actor).send(&Start)?;
        }

        Ok(())
    }
}

/// An actor of the big program: it pings a neighbour picked at random, and on each answer pings again until it has
/// sent all its pings. It answers every ping it is sent.
#[derive(Bound)]
#[kind(name = "aether.savina.pinging")]
struct Pinging {
    neighbours: Vec<ActorRef<Self>>,
    /// The state of its random numbers.
    random: u64,
}

impl Pinging {
    /// Pings a neighbour picked at random, with `left` to go after it.
    fn ping<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, left: u64) -> Result<(), Fault>
    where
        S: Allows<ToType<Self, Ping>, I>,
    {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        let count = u64::try_from(self.neighbours.len()).unwrap_or(1).max(1);
        let picked = usize::try_from(self.random % count).unwrap_or(0);
        turn.to(self.neighbours[picked]).send(&Ping { left })?;

        Ok(())
    }
}

impl ChildOf for Pinging {
    type Parent = Crowd;
}

impl Actor for Pinging {
    type Config = Number;
    type Key = u64;
    type Handles = (Neighbour, Start, Ping, Pong, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { neighbours: Vec::new(), random: config.number * 0x9e37_79b9_7f4a_7c15 + 1 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Neighbour> for Pinging {
    type Sends = ();

    fn handle(&mut self, Neighbour { other }: Neighbour, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.neighbours.push(other);

        Ok(())
    }
}

impl Handles<Start> for Pinging {
    type Sends = (ToType<Self, Ping>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.ping(turn, BIG_PINGS - 1)
    }
}

impl Handles<Ping> for Pinging {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { left })?;

        Ok(())
    }
}

impl Handles<Pong> for Pinging {
    type Sends = (ToType<Self, Ping>,);

    fn handle(&mut self, Pong { left }: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if left > 0 {
            self.ping(turn, left - 1)?;
        }

        Ok(())
    }
}

/// A request for the Fibonacci number at `number`.
#[derive(Plain)]
#[kind(name = "aether.savina.request")]
struct Request {
    number: u64,
}

/// A Fibonacci number, answering a [`Request`].
#[derive(Plain)]
#[kind(name = "aether.savina.response")]
struct Response {
    value: u64,
}

/// The root of the fib program: it spawns the first actor, and once it is ready asks it for the number.
#[derive(Plain)]
#[kind(name = "aether.savina.fib_root")]
struct FibRoot;

impl ChildOf for FibRoot {
    type Parent = Top;
}

impl Actor for FibRoot {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready, Response);
    type BeginSends = (ToType<Fib, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let first: ActorRef<Fib> = spawned(turn, 0);
        ready(turn, &[first])?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for FibRoot {
    type Sends = (ToSender<Request>,);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Request { number: FIB })?;

        Ok(())
    }
}

/// The first actor's answer, which ends the program.
impl Handles<Response> for FibRoot {
    type Sends = ();

    fn handle(&mut self, _: Response, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor of the fib program: asked for a number, it answers at once if it is one of the first two, and otherwise
/// spawns a child for each of the two before it and answers with their sum. It closes once it has answered.
#[derive(Plain)]
#[kind(name = "aether.savina.fib")]
struct Fib {
    sum: u64,
    answers: u64,
}

/// An answer to a parent, which is another actor of the fib program or its root: any actor that handles a
/// [`Response`].
type Answer = ToAny<(Response,), Response>;

impl ChildOf for Fib {
    type Parent = Anywhere;
}

impl Actor for Fib {
    type Config = Number;
    type Key = u64;
    type Handles = (Request, Response, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { sum: 0, answers: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Request> for Fib {
    type Sends = (ToType<Self, Request>, Answer);

    fn handle(&mut self, Request { number }: Request, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if number <= 2 {
            turn.to(parent()).send(&Response { value: 1 })?;
            turn.close();
        } else {
            for (key, before) in [(0, number - 1), (1, number - 2)] {
                let Ok(child) = turn.spawn(&key, Number { number: key }) else {
                    panic!("a child begins");
                };
                turn.to(child).send(&Request { number: before })?;
            }
        }

        Ok(())
    }
}

impl Handles<Response> for Fib {
    type Sends = (Answer,);

    fn handle(&mut self, Response { value }: Response, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.sum += value;
        self.answers += 1;
        if self.answers == 2 {
            turn.to(parent()).send(&Response { value: self.sum })?;
            turn.close();
        }

        Ok(())
    }
}

/// A number for the sieve to test.
#[derive(Plain)]
#[kind(name = "aether.savina.candidate")]
struct Candidate {
    number: u64,
}

/// The word that the sieve has every number: each filter passes it on and closes.
#[derive(Plain)]
#[kind(name = "aether.savina.exit")]
struct Exit;

/// The root of the sieve program: it spawns the first filter, which keeps 2, and once it is ready sends it every
/// odd number up to the limit, then the word to stop.
#[derive(Plain)]
#[kind(name = "aether.savina.sieve")]
struct Sieve;

impl ChildOf for Sieve {
    type Parent = Top;
}

impl Actor for Sieve {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready,);
    type BeginSends = (ToType<Filter, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(first) = turn.spawn(&0, Number { number: 2 }) else {
            panic!("the first filter begins");
        };
        ready(turn, &[first])?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Sieve {
    type Sends = (ToSender<Candidate>, ToSender<Exit>);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for number in (3..=SIEVE_LIMIT).step_by(2) {
            turn.reply(&Candidate { number })?;
        }
        turn.reply(&Exit)?;

        Ok(())
    }
}

/// What follows a filter in the sieve's pipeline.
#[derive(Bound)]
#[kind(name = "aether.savina.next")]
enum Next {
    /// Nothing: it is the last filter, and keeps a number no filter divides.
    Last,
    /// The filter it spawned when it was full, which it passes every number on to.
    Filter(ActorRef<Filter>),
}

/// A filter of the sieve: it drops a number one of its primes divides, and keeps or passes on the rest.
#[derive(Bound)]
#[kind(name = "aether.savina.filter")]
struct Filter {
    primes: Vec<u64>,
    next: Next,
}

impl ChildOf for Filter {
    type Parent = Anywhere;
}

impl Actor for Filter {
    type Config = Number;
    type Key = u64;
    type Handles = (Candidate, Exit, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(first: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { primes: vec![first.number], next: Next::Last })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Candidate> for Filter {
    type Sends = (ToType<Self, Candidate>,);

    fn handle(&mut self, Candidate { number }: Candidate, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let divided = self.primes.iter().any(|prime| number % prime == 0);
        if divided {
            return Ok(());
        }
        match self.next {
            Next::Filter(next) => {
                turn.to(next).send(&Candidate { number })?;
            }
            Next::Last if self.primes.len() < SIEVE_KEPT => self.primes.push(number),
            Next::Last => {
                let Ok(next) = turn.spawn(&0, Number { number }) else {
                    panic!("a filter begins");
                };
                self.next = Next::Filter(next);
            }
        }

        Ok(())
    }
}

impl Handles<Exit> for Filter {
    type Sends = (ToType<Self, Exit>,);

    fn handle(&mut self, _: Exit, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Next::Filter(next) = self.next {
            turn.to(next).send(&Exit)?;
        }
        turn.close();

        Ok(())
    }
}

/// One more hop of a churn actor's life, with this many to go after it.
#[derive(Plain)]
#[kind(name = "aether.savina.live")]
struct Live {
    left: u64,
}

/// The word that a churn actor has lived its hops and is closing.
#[derive(Plain)]
#[kind(name = "aether.savina.died")]
struct Died;

/// The root of the churn program: it keeps its population alive, spawning one in the place of each that closes,
/// until the total has lived.
#[derive(Plain)]
#[kind(name = "aether.savina.churn")]
struct Churn {
    spawned: u64,
    /// How many roots share the program: the population and the total are each this one's share of them.
    spawners: u64,
}

impl Churn {
    /// Spawns the next actor and starts its life.
    fn spawn_next<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Mortal, Live>, I>,
    {
        let Ok(child) = turn.spawn::<Mortal>(&self.spawned, Number { number: self.spawned }) else {
            panic!("a churn actor begins");
        };
        self.spawned += 1;
        turn.to(child).send(&Live { left: CHURN_LIFE })?;

        Ok(())
    }
}

impl ChildOf for Churn {
    type Parent = Top;
}

impl Actor for Churn {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready, Died);
    type BeginSends = (ToType<Mortal, Ready>,);
    type EndSends = ();

    /// A root configured with `number` shares the program among that many roots, none and one alike being the whole.
    fn begin(config: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let first: ActorRef<Mortal> = spawned(turn, 0);
        ready(turn, &[first])?;

        Ok(Self { spawned: 1, spawners: config.number.max(1) })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Churn {
    type Sends = (ToSender<Live>, ToType<Mortal, Live>);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Live { left: CHURN_LIFE })?;
        while self.spawned < CHURN_ALIVE / self.spawners {
            self.spawn_next(turn)?;
        }

        Ok(())
    }
}

impl Handles<Died> for Churn {
    type Sends = (ToType<Mortal, Live>,);

    fn handle(&mut self, _: Died, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.spawned < CHURN_TOTAL / self.spawners {
            self.spawn_next(turn)?;
        }

        Ok(())
    }
}

/// An actor of the churn program: it lives a number of hops of mail to itself, then tells its parent and closes.
#[derive(Plain)]
#[kind(name = "aether.savina.mortal")]
struct Mortal;

impl ChildOf for Mortal {
    type Parent = Churn;
}

impl Actor for Mortal {
    type Config = Number;
    type Key = u64;
    type Handles = (Live, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Live> for Mortal {
    type Sends = (ToType<Self, Live>, ToType<Churn, Died>);

    fn handle(&mut self, Live { left }: Live, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if left > 0 {
            turn.to(turn.me()).send(&Live { left: left - 1 })?;
        } else {
            turn.to(turn.parent()).send(&Died)?;
            turn.close();
        }

        Ok(())
    }
}

/// A frame of the world program for a body, with the bodies either side of it to nudge.
#[derive(Bound)]
#[kind(name = "aether.savina.frame")]
struct Frame {
    left: ActorRef<Body>,
    right: ActorRef<Body>,
}

/// A push from the body beside it.
#[derive(Plain)]
#[kind(name = "aether.savina.nudge")]
struct Nudge {
    push: f32,
}

/// A body's answer to its frame, its life going on.
#[derive(Plain)]
#[kind(name = "aether.savina.moved")]
struct Moved;

/// A body's answer to its frame, its last: the body in place `place` has lived its frames and is closing.
#[derive(Plain)]
#[kind(name = "aether.savina.expired")]
struct Expired {
    place: u64,
}

/// The word that the world program is over: each body closes.
#[derive(Plain)]
#[kind(name = "aether.savina.over")]
struct Over;

/// A region of the world program, and one of its roots: it keeps its bodies in a row of places, and runs them a
/// frame at a time, sending each its frame and starting the next once every body has answered. It puts a new body in
/// the place of each that expires, and once the frames are run tells its bodies the world is over, and closes.
#[derive(Bound)]
#[kind(name = "aether.savina.region")]
struct Region {
    places: Vec<ActorRef<Body>>,
    frame: u64,
    answered: u64,
    spawned: u64,
}

impl Region {
    /// Sends every body its frame, or, once the frames are run, the word that the world is over, and closes.
    fn next_frame<S, I, J>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Body, Frame>, I> + Allows<ToType<Body, Over>, J>,
    {
        self.answered = 0;
        if self.frame == WORLD_FRAMES {
            for body in &self.places {
                turn.to(*body).send(&Over)?;
            }
            turn.close();
            WORLD_OVER.fetch_add(1, Ordering::Relaxed);

            return Ok(());
        }

        let count = self.places.len();
        for (place, body) in self.places.iter().enumerate() {
            let left = self.places[(place + count - 1) % count];
            let right = self.places[(place + 1) % count];
            turn.to(*body).send(&Frame { left, right })?;
        }
        self.frame += 1;

        Ok(())
    }

    /// Counts a body's answer, and starts the next frame once every body has answered.
    fn answer<S, I, J>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Body, Frame>, I> + Allows<ToType<Body, Over>, J>,
    {
        self.answered += 1;
        let is_last = self.answered == WORLD_BODIES;
        if is_last {
            self.next_frame(turn)?;
        }

        Ok(())
    }
}

impl ChildOf for Region {
    type Parent = Top;
}

impl Actor for Region {
    type Config = Number;
    type Key = u64;
    type Handles = (Ready, Moved, Expired);
    type BeginSends = (ToType<Body, Ready>,);
    type EndSends = ();

    fn begin(_: Number, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let places: Vec<ActorRef<Body>> = (0..WORLD_BODIES).map(|place| spawned(turn, place)).collect();
        ready(turn, &places)?;

        Ok(Self { places, frame: 0, answered: 0, spawned: WORLD_BODIES })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ready> for Region {
    type Sends = (ToType<Body, Frame>, ToType<Body, Over>);

    fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.next_frame(turn)
    }
}

impl Handles<Moved> for Region {
    type Sends = (ToType<Body, Frame>, ToType<Body, Over>);

    fn handle(&mut self, _: Moved, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.answer(turn)
    }
}

impl Handles<Expired> for Region {
    type Sends = (ToType<Body, Frame>, ToType<Body, Over>);

    /// Puts a new body, with a whole life, in the expired one's place, unless the world is over after this frame.
    fn handle(&mut self, Expired { place }: Expired, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.frame < WORLD_FRAMES {
            let Ok(body) = turn.spawn(&self.spawned, Number { number: (WORLD_LIFE << 32) | place }) else {
                panic!("a body begins");
            };
            self.spawned += 1;
            let Ok(place) = usize::try_from(place) else {
                panic!("a place is an index of the row");
            };
            self.places[place] = body;
        }

        self.answer(turn)
    }
}

/// A body of the world program: each frame it moves, pushed by what its neighbours nudged it with, nudges them in
/// turn, and answers its region. It lives a number of frames, its last answer saying it has expired.
#[derive(Plain)]
#[kind(name = "aether.savina.body")]
struct Body {
    place: u64,
    left: u64,
    position: f32,
    velocity: f32,
    pushed: f32,
}

impl ChildOf for Body {
    type Parent = Region;
}

impl Actor for Body {
    type Config = Number;
    type Key = u64;
    type Handles = (Frame, Nudge, Over, Ready);
    type BeginSends = ();
    type EndSends = ();

    /// A body made from `number` has the frames it lives in its upper half, and its place in the lower; a body made
    /// as its region begins, numbered by its place alone, lives a share of a life, so that bodies expire a few a
    /// frame and not all at once.
    fn begin(Number { number }: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let place = number & u64::from(u32::MAX);
        let left = match number >> 32 {
            0 => place % WORLD_LIFE + 1,
            lived => lived,
        };
        let at = place as f32;

        Ok(Self { place, left, position: at, velocity: 1.0 - at / WORLD_BODIES as f32, pushed: 0.0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Frame> for Body {
    type Sends = (ToType<Self, Nudge>, ToType<Region, Moved>, ToType<Region, Expired>);

    fn handle(&mut self, Frame { left, right }: Frame, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.velocity = (self.velocity + self.pushed) * 0.99;
        self.position += self.velocity;
        let is_out = self.position.abs() > WORLD_BODIES as f32;
        if is_out {
            self.velocity = -self.velocity;
        }
        self.pushed = 0.0;
        turn.to(left).send(&Nudge { push: -0.01 * self.velocity })?;
        turn.to(right).send(&Nudge { push: 0.01 * self.velocity })?;

        self.left -= 1;
        if self.left == 0 {
            turn.to(turn.parent()).send(&Expired { place: self.place })?;
            turn.close();
        } else {
            turn.to(turn.parent()).send(&Moved)?;
        }

        Ok(())
    }
}

impl Handles<Nudge> for Body {
    type Sends = ();

    fn handle(&mut self, Nudge { push }: Nudge, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.pushed += push;

        Ok(())
    }
}

impl Handles<Over> for Body {
    type Sends = ();

    fn handle(&mut self, _: Over, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// How many actors and pieces of mail a program makes.
struct Size {
    actors: u64,
    mails: u64,
}

/// The fib program's size: an actor for each number asked for, each asked once and answering once, beside the
/// root and its start.
fn fib_size() -> Size {
    let mut asked = 0;
    let mut stack = vec![FIB];
    while let Some(number) = stack.pop() {
        asked += 1;
        if number > 2 {
            stack.extend([number - 1, number - 2]);
        }
    }

    Size { actors: asked + 1, mails: 2 * asked + 2 }
}

/// The sieve program's size, found by running the same sieve without actors: every number sent and passed on, the
/// word to stop passed down every filter, and the root's start.
fn sieve_size() -> Size {
    let mut filters: Vec<Vec<u64>> = vec![vec![2]];
    let mut mails = 2;
    for number in (3..=SIEVE_LIMIT).step_by(2) {
        mails += 1;
        let mut place = 0;
        loop {
            let divided = filters[place].iter().any(|prime| number % prime == 0);
            if divided {
                break;
            }
            let is_last = place + 1 == filters.len();
            if !is_last {
                mails += 1;
                place += 1;
                continue;
            }
            if filters[place].len() < SIEVE_KEPT {
                filters[place].push(number);
            } else {
                filters.push(vec![number]);
            }
            break;
        }
    }
    let count = u64::try_from(filters.len()).unwrap_or(u64::MAX);

    Size { actors: count + 1, mails: mails + count }
}

/// Tells the last of `children`, each of type `T`, that the program can start, so it answers from a turn.
fn ready<Me, T: Handles<Ready>, S, I>(turn: &mut Beginning<'_, Me, S>, children: &[ActorRef<T>]) -> Result<(), Fault>
where
    S: Allows<ToType<T, Ready>, I>,
{
    if let Some(last) = children.last() {
        turn.to(*last).send(&Ready)?;
    }

    Ok(())
}

/// Each of `actors` answers the word that a program can start, to the root that sent it.
macro_rules! answers_ready {
    ($($actor:ty),*) => {$(
        impl Handles<Ready> for $actor {
            type Sends = (ToSender<Ready>,);

            fn handle(&mut self, _: Ready, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                turn.reply(&Ready)?;

                Ok(())
            }
        }
    )*};
}

answers_ready!(Ponger, Counter, Forked, Pinging, Fib, Filter, Mortal, Body);

/// A child of type `A` numbered `number`, spawned by `turn`.
fn spawned<Me, A, S>(turn: &mut Beginning<'_, Me, S>, number: u64) -> ActorRef<A>
where
    A: ChildOf + Actor<Config = Number, Key = u64>,
    A::Parent: Admits<Me, A>,
{
    let Ok(child) = turn.spawn(&number, Number { number }) else {
        panic!("a child begins");
    };

    child
}

/// One program: its name, how many actors it runs, how many pieces of mail it sends, and how to start it.
struct Program {
    name: &'static str,
    size: fn() -> Size,
    /// Adds the program's roots for its run numbered by the second argument, under keys no earlier run used.
    start: fn(&mut Engine, u64),
    /// Stops the benchmark unless the run just done did all its work, for a program whose actors would otherwise go
    /// idle unseen when mail they wait on is lost.
    check: fn(),
}

/// Checks nothing, for a program whose actors check their own work as they close.
fn unchecked() {}

/// Adds the root of type `A` for the run numbered `round`, which starts a program.
fn root<A: Root + Actor<Config = Number, Key = u64>>(engine: &mut Engine, round: u64) {
    let Ok(_) = engine.add_root::<A>(&round, Number { number: 0 }) else {
        panic!("the root begins");
    };
}

/// The roots of the wide churn program, each with its share of it.
fn churn_wide(engine: &mut Engine, round: u64) {
    for key in 0..CHURN_SPAWNERS {
        let Ok(_) = engine.add_root::<Churn>(&(round * CHURN_SPAWNERS + key), Number { number: CHURN_SPAWNERS }) else {
            panic!("a root begins");
        };
    }
}

/// The roots of the world program, its regions.
fn world(engine: &mut Engine, round: u64) {
    for key in 0..WORLD_REGIONS {
        let Ok(_) = engine.add_root::<Region>(&(round * WORLD_REGIONS + key), Number { number: key }) else {
            panic!("a region begins");
        };
    }
}

/// How many regions of the world program have run every frame since its last run was checked.
static WORLD_OVER: AtomicU64 = AtomicU64::new(0);

/// Stops the benchmark unless every region of the world program's last run ran every frame: a region waiting on a
/// body's answer that was lost goes idle unseen, and the run looks quicker.
fn world_check() {
    let over = WORLD_OVER.swap(0, Ordering::Relaxed);
    assert_eq!(over, WORLD_REGIONS, "every region of the world runs every frame");
}

/// The world program's size, found by running its lives without actors: each region, its bodies, and one in the
/// place of each that expires before the last frame; and for each region its start, every frame's mail to each
/// body, the two nudges each body sends and its answer, and the word that the world is over.
fn world_size() -> Size {
    let mut lives: Vec<u64> = (0..WORLD_BODIES).map(|place| place % WORLD_LIFE + 1).collect();
    let mut replaced = 0;
    for frame in 0..WORLD_FRAMES {
        for life in &mut lives {
            *life -= 1;
            let is_replaced = *life == 0 && frame + 1 < WORLD_FRAMES;
            if is_replaced {
                replaced += 1;
                *life = WORLD_LIFE;
            }
        }
    }

    Size {
        actors: WORLD_REGIONS * (1 + WORLD_BODIES + replaced),
        mails: WORLD_REGIONS * (2 + 4 * WORLD_FRAMES * WORLD_BODIES + WORLD_BODIES),
    }
}

/// The wide churn program's size: each root's share of the churn program, and the roots.
fn churn_wide_size() -> Size {
    let lived = CHURN_TOTAL / CHURN_SPAWNERS * CHURN_SPAWNERS;

    Size { actors: lived + CHURN_SPAWNERS, mails: lived * (CHURN_LIFE + 2) + 2 * CHURN_SPAWNERS }
}

const PROGRAMS: [Program; 10] = [
    Program {
        name: "pingpong",
        size: || Size { actors: 2, mails: 2 * PINGS },
        start: root::<Pinger>,
        check: unchecked,
    },
    Program {
        name: "count",
        size: || Size { actors: 2, mails: COUNTS + 2 },
        start: root::<Producer>,
        check: unchecked,
    },
    Program {
        name: "count-begin",
        size: || Size { actors: 2, mails: COUNTS + 2 },
        start: root::<BeginProducer>,
        check: unchecked,
    },
    Program {
        name: "fjthrput",
        size: || Size { actors: FORKS + 1, mails: FORKS * FORKED },
        start: root::<Forker>,
        check: unchecked,
    },
    Program {
        name: "big",
        size: || Size {
            actors: BIG_ACTORS + 1,
            mails: BIG_ACTORS * (BIG_ACTORS - 1) + BIG_ACTORS + 2 * BIG_ACTORS * BIG_PINGS,
        },
        start: root::<Crowd>,
        check: unchecked,
    },
    Program { name: "fib", size: fib_size, start: root::<FibRoot>, check: unchecked },
    Program { name: "sieve", size: sieve_size, start: root::<Sieve>, check: unchecked },
    Program {
        name: "churn",
        size: || Size { actors: CHURN_TOTAL + 1, mails: CHURN_TOTAL * (CHURN_LIFE + 2) + 2 },
        start: root::<Churn>,
        check: unchecked,
    },
    Program { name: "churn-wide", size: churn_wide_size, start: churn_wide, check: unchecked },
    Program { name: "world", size: world_size, start: world, check: world_check },
];

/// One run of `program` on `workers` workers, each part running at most `hops` fused hops a step: the nanoseconds it
/// took, from its root's `begin` to the last step, the steps, and how its sends fared while parts could fuse. The host
/// is made before the clock starts, so what it measures as it starts is not counted.
fn run(program: &Program, workers: NonZeroUsize, hops: usize) -> (u64, u64, Hops) {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let engine = Engine::new(parts, 1024, 1 << 30, 1 << 30, hops).ready();
    let mut headless = Headless::new(engine, workers);
    let clock = Monotonic::new();
    let began = clock.nanos();
    (program.start)(headless.engine_mut(), 0);
    headless.run_until_idle();
    (program.check)();

    (clock.nanos() - began, headless.measured().steps, headless.engine().hopped())
}

/// `rounds` runs of `program` one after another on one engine and one host, on `workers` workers, each part running
/// at most `hops` fused hops a step: the nanoseconds each round took, from its roots' `begin` to the last step. What
/// the engine learned in a round, of what turns cost, where parts run, and what splitting costs, it keeps for the
/// next, as a long-running engine does.
fn steady(program: &Program, workers: NonZeroUsize, hops: usize, rounds: u64) -> Vec<u64> {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let engine = Engine::new(parts, 1024, 1 << 30, 1 << 30, hops).ready();
    let mut headless = Headless::new(engine, workers);

    (0..rounds)
        .map(|round| {
            let clock = Monotonic::new();
            let began = clock.nanos();
            (program.start)(headless.engine_mut(), round);
            headless.run_until_idle();
            (program.check)();

            clock.nanos() - began
        })
        .collect()
}

/// Prints `program`'s rounds of a steady run, `took` nanoseconds each: the first round's cost of a piece of mail,
/// cold; the median and best of the rounds after the first third, once the engine has learned; and the first round
/// within a twentieth of that median, where it settled.
fn print_steady(program: &Program, size: &Size, workers: NonZeroUsize, took: &[u64]) {
    let per = |nanos: u64| nanos / size.mails.max(1);
    let warm = (took.len() / 3).max(1).min(took.len().saturating_sub(1));
    let mut settled: Vec<u64> = took[warm..].to_vec();
    settled.sort_unstable();
    let median = settled.get(settled.len() / 2).copied().unwrap_or_default();
    let best = settled.first().copied().unwrap_or_default();
    let within = median + median / 20;
    let settles = took.iter().position(|nanos| *nanos <= within).map_or(0, |round| round + 1);
    println!(
        "{:11} {:6} {workers:7} {:10} {:9} {:9} {:9} {settles:8}",
        program.name,
        size.actors,
        size.mails,
        per(took[0]),
        per(median),
        per(best),
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let named: Vec<&str> = args.iter().filter(|arg| !arg.contains('=')).map(String::as_str).collect();
    let workers: Vec<usize> = args
        .iter()
        .find_map(|arg| arg.strip_prefix("workers="))
        .map_or_else(|| vec![1, 4, 16], |given| given.split(',').filter_map(|count| count.parse().ok()).collect());
    let hops = args.iter().find_map(|arg| arg.strip_prefix("hops=")).and_then(|given| given.parse().ok()).unwrap_or(0);
    let rounds: u64 =
        args.iter().find_map(|arg| arg.strip_prefix("rounds=")).and_then(|given| given.parse().ok()).unwrap_or(0);

    if rounds > 0 {
        println!("program     actors workers      mails   cold ns  steady ns   best ns  settled");
        for program in &PROGRAMS {
            let is_skipped = !named.is_empty() && !named.contains(&program.name);
            if is_skipped {
                continue;
            }
            for &count in &workers {
                let workers = NonZeroUsize::MIN.saturating_add(count.max(1) - 1);
                let took = steady(program, workers, hops, rounds.max(2));
                print_steady(program, &(program.size)(), workers, &took);
            }
        }
        return;
    }

    println!("program     actors workers      mails   M mails/s  ns/mail    steps");
    for program in &PROGRAMS {
        let is_skipped = !named.is_empty() && !named.contains(&program.name);
        if is_skipped {
            continue;
        }
        let size = (program.size)();
        for &count in &workers {
            let workers = NonZeroUsize::MIN.saturating_add(count.max(1) - 1);
            let runs = (0..3).map(|_| run(program, workers, hops));
            let (took, steps, hopped) = runs.min_by_key(|(took, _, _)| *took).unwrap_or_default();
            println!(
                "{:11} {:6} {workers:7} {:10}   {:9.2} {:8} {steps:8}",
                program.name,
                size.actors,
                size.mails,
                size.mails as f64 * 1e3 / took.max(1) as f64,
                took / size.mails.max(1),
            );
            if hops > 0 {
                let Hops { fused, spilled, unseated, elsewhere, ended, burst, closing } = hopped;
                println!(
                    "            fused={fused} spilled={spilled} unseated={unseated} \
                     elsewhere={elsewhere} ended={ended} burst={burst} closing={closing}"
                );
            }
        }
    }
}
