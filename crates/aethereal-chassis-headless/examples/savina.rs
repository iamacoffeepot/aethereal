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
//! And four of requests (ADR-0017): 64 askers each ask 4,096 questions, 64 at once, the next 64 once all are
//! answered, of one keeper (`-one`) or spread over 16 (`-children`), and the root tells each asker to start as it
//! begins. In `ask-*` the keeper answers each question in the turn that handles it; in `hold-*` it holds each, asks a
//! stamper for a stamp, and answers what it held when the stamp comes back, so `hold-one`'s keeper holds up to all
//! 4,096 of a round at once. A request is two pieces of mail in `ask-*`, and four in `hold-*`.
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

use aethereal_actor::guard::Child;
use aethereal_actor::{
    Actor, ActorRef, Admits, Answered, Anywhere, Beginning, ChildOf, Fault, Guard, Handles, Held, Later, ListsChild,
    PlacesBeneath, Root, Spawn, Speaks, Turn, actor, parent,
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
/// How many askers the request programs have, how many questions each asks, and how many it asks at once.
const ASKERS: u64 = 64;
const ASKED: u64 = 4_096;
const ASKED_AT_ONCE: u64 = 64;
/// How many keepers the request programs that split their answering among children have.
const KEEPERS: u64 = 16;

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

#[actor(key = u64, spawns(Ponger))]
impl Pinger {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Ok(ponger) = turn.spawn::<Ponger>(&0, Number { number: 0 }) else {
            panic!("the pong side begins");
        };
        turn.to(ponger).send(&Ready)?;

        Ok(Self)
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Child<Ponger>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ping { left: PINGS - 1 })?;

        Ok(())
    }

    #[tell]
    fn pong(&mut self, Pong { left }: Pong, turn: &mut Turn<'_, Self, Child<Ponger>>) -> Result<(), Fault> {
        if left > 0 {
            turn.to(*turn.from()).send(&Ping { left: left - 1 })?;
        }

        Ok(())
    }
}

/// The pong side of the ping-pong program: it tells the ping side a pong for each ping.
#[derive(Plain)]
#[kind(name = "aether.savina.ponger")]
struct Ponger;

#[actor(key = u64, parent = Pinger)]
impl Ponger {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, ActorRef<Pinger>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong { left })?;

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

        Ok(())
    }
}

/// The producer of the counting program, and its root: it spawns the counter, and once the counter is ready sends
/// it everything to count, then asks for the count.
#[derive(Plain)]
#[kind(name = "aether.savina.producer")]
struct Producer;

#[actor(key = u64, spawns(Counter))]
impl Producer {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn::<Counter>(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        turn.to(counter).send(&Ready)?;

        Ok(Self)
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Child<Counter>>) -> Result<(), Fault> {
        for _ in 0..COUNTS {
            turn.to(*turn.from()).send(&Increment)?;
        }
        turn.to(*turn.from()).send(&Retrieve)?;

        Ok(())
    }

    #[tell]
    fn total(&mut self, Total { count }: Total) -> Result<(), Fault> {
        assert_eq!(count, COUNTS, "the counter counts every piece of mail");

        Ok(())
    }
}

/// The counting program with its producer sending everything from its `begin`, between steps: what a root that
/// sends a great deal as it begins costs.
#[derive(Plain)]
#[kind(name = "aether.savina.begin_producer")]
struct BeginProducer;

#[actor(key = u64, spawns(Counter))]
impl BeginProducer {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Ok(counter) = turn.spawn::<Counter>(&0, Number { number: 0 }) else {
            panic!("the counter begins");
        };
        for _ in 0..COUNTS {
            turn.to(counter).send(&Increment)?;
        }
        turn.to(counter).send(&Retrieve)?;

        Ok(Self)
    }

    #[tell]
    fn total(&mut self, Total { count }: Total) -> Result<(), Fault> {
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

#[actor(key = u64, parent = Anywhere)]
impl Counter {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self { count: 0 })
    }

    #[tell]
    fn increment(&mut self, _: Increment) -> Result<(), Fault> {
        self.count += 1;

        Ok(())
    }

    #[tell]
    fn retrieve(&mut self, _: Retrieve, turn: &mut Turn<'_, Self, Speaks<(Total,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Total { count: self.count })?;

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

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

#[actor(key = u64, spawns(Forked))]
impl Forker {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let workers: Vec<ActorRef<Forked>> = (0..FORKS).map(|number| spawned(turn, number)).collect();
        ready(turn, &workers)?;

        Ok(Self { workers })
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

#[actor(key = u64, parent = Forker)]
impl Forked {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn work(&mut self, _: Work) -> Result<(), Fault> {
        let sine = hint::black_box(37.2_f64).sin();
        hint::black_box(sine * sine);

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

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

#[actor(key = u64, spawns(Pinging))]
impl Crowd {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let actors: Vec<ActorRef<Pinging>> = (0..BIG_ACTORS).map(|number| spawned(turn, number)).collect();
        ready(turn, &actors)?;

        Ok(Self { actors })
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        for actor in &self.actors {
            for other in self.actors.iter().filter(|other| *other != actor) {
                turn.to(*actor).send(&Neighbour { other: *other })?;
            }
            turn.to(*actor).send(&Start)?;
        }

        Ok(())
    }
}

/// An actor of the big program: it pings a neighbour picked at random, and on each pong pings again until it has
/// sent all its pings. It tells a pong to the sender of every ping it is sent.
#[derive(Bound)]
#[kind(name = "aether.savina.pinging")]
struct Pinging {
    neighbours: Vec<ActorRef<Self>>,
    /// The state of its random numbers.
    random: u64,
}

#[actor(key = u64, parent = Crowd)]
impl Pinging {
    fn begin(config: Number) -> Result<Self, Fault> {
        Ok(Self { neighbours: Vec::new(), random: config.number * 0x9e37_79b9_7f4a_7c15 + 1 })
    }

    #[tell]
    fn neighbour(&mut self, Neighbour { other }: Neighbour) -> Result<(), Fault> {
        self.neighbours.push(other);

        Ok(())
    }

    #[tell]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.ping(turn, BIG_PINGS - 1)
    }

    #[tell]
    fn pinged(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, ActorRef<Self>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong { left })?;

        Ok(())
    }

    #[tell]
    fn pong(&mut self, Pong { left }: Pong, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if left > 0 {
            self.ping(turn, left - 1)?;
        }

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

        Ok(())
    }

    /// Pings a neighbour picked at random, with `left` to go after it.
    fn ping(&mut self, turn: &mut Turn<'_, Self>, left: u64) -> Result<(), Fault> {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        let count = u64::try_from(self.neighbours.len()).unwrap_or(1).max(1);
        let picked = usize::try_from(self.random % count).unwrap_or(0);
        turn.to(self.neighbours[picked]).send(&Ping { left })?;

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

#[actor(key = u64, spawns(Fib))]
impl FibRoot {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let first: ActorRef<Fib> = spawned(turn, 0);
        ready(turn, &[first])?;

        Ok(Self)
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Child<Fib>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Request { number: FIB })?;

        Ok(())
    }

    /// The first actor's answer, which ends the program.
    #[tell]
    fn response(&mut self, _: Response) -> Result<(), Fault> {
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

#[actor(key = u64, parent = Anywhere, spawns(Self))]
impl Fib {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self { sum: 0, answers: 0 })
    }

    #[tell]
    fn request(&mut self, Request { number }: Request, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if number <= 2 {
            turn.to(parent::<(Response,)>()).send(&Response { value: 1 })?;
            turn.close();
        } else {
            for (key, before) in [(0, number - 1), (1, number - 2)] {
                let Ok(child) = turn.spawn::<Self>(&key, Number { number: key }) else {
                    panic!("a child begins");
                };
                turn.to(child).send(&Request { number: before })?;
            }
        }

        Ok(())
    }

    #[tell]
    fn response(&mut self, Response { value }: Response, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.sum += value;
        self.answers += 1;
        if self.answers == 2 {
            turn.to(parent::<(Response,)>()).send(&Response { value: self.sum })?;
            turn.close();
        }

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

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

#[actor(key = u64, spawns(Filter))]
impl Sieve {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Ok(first) = turn.spawn::<Filter>(&0, Number { number: 2 }) else {
            panic!("the first filter begins");
        };
        ready(turn, &[first])?;

        Ok(Self)
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Child<Filter>>) -> Result<(), Fault> {
        for number in (3..=SIEVE_LIMIT).step_by(2) {
            turn.to(*turn.from()).send(&Candidate { number })?;
        }
        turn.to(*turn.from()).send(&Exit)?;

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

#[actor(key = u64, parent = Anywhere, spawns(Self))]
impl Filter {
    fn begin(first: Number) -> Result<Self, Fault> {
        Ok(Self { primes: vec![first.number], next: Next::Last })
    }

    #[tell]
    fn candidate(&mut self, Candidate { number }: Candidate, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

    #[tell]
    fn exit(&mut self, _: Exit, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Next::Filter(next) = self.next {
            turn.to(next).send(&Exit)?;
        }
        turn.close();

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

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

#[actor(key = u64, spawns(Mortal))]
impl Churn {
    /// A root configured with `number` shares the program among that many roots, none and one alike being the whole.
    fn begin(config: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let first: ActorRef<Mortal> = spawned(turn, 0);
        ready(turn, &[first])?;

        Ok(Self { spawned: 1, spawners: config.number.max(1) })
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Child<Mortal>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Live { left: CHURN_LIFE })?;
        while self.spawned < CHURN_ALIVE / self.spawners {
            self.spawn_next(turn)?;
        }

        Ok(())
    }

    #[tell]
    fn died(&mut self, _: Died, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if self.spawned < CHURN_TOTAL / self.spawners {
            self.spawn_next(turn)?;
        }

        Ok(())
    }

    /// Spawns the next actor and starts its life, in a turn whose guard is `F`.
    fn spawn_next<F: Guard<Self>>(&mut self, turn: &mut Turn<'_, Self, F>) -> Result<(), Fault> {
        let Ok(child) = turn.spawn::<Mortal>(&self.spawned, Number { number: self.spawned }) else {
            panic!("a churn actor begins");
        };
        self.spawned += 1;
        turn.to(child).send(&Live { left: CHURN_LIFE })?;

        Ok(())
    }
}

/// An actor of the churn program: it lives a number of hops of mail to itself, then tells its parent and closes.
#[derive(Plain)]
#[kind(name = "aether.savina.mortal")]
struct Mortal;

#[actor(key = u64, parent = Churn)]
impl Mortal {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn live(&mut self, Live { left }: Live, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if left > 0 {
            turn.to(turn.me()).send(&Live { left: left - 1 })?;
        } else {
            turn.to(turn.parent()).send(&Died)?;
            turn.close();
        }

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

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

#[actor(key = u64, spawns(Body))]
impl Region {
    fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let places: Vec<ActorRef<Body>> = (0..WORLD_BODIES).map(|place| spawned(turn, place)).collect();
        ready(turn, &places)?;

        Ok(Self { places, frame: 0, answered: 0, spawned: WORLD_BODIES })
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.next_frame(turn)
    }

    #[tell]
    fn moved(&mut self, _: Moved, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.answer(turn)
    }

    /// Puts a new body, with a whole life, in the expired one's place, unless the world is over after this frame.
    #[tell]
    fn expired(&mut self, Expired { place }: Expired, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

    /// Sends every body its frame, or, once the frames are run, the word that the world is over, and closes.
    fn next_frame(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
    fn answer(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.answered += 1;
        let is_last = self.answered == WORLD_BODIES;
        if is_last {
            self.next_frame(turn)?;
        }

        Ok(())
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

#[actor(key = u64, parent = Region)]
impl Body {
    /// A body made from `number` has the frames it lives in its upper half, and its place in the lower; a body made
    /// as its region begins, numbered by its place alone, lives a share of a life, so that bodies expire a few a
    /// frame and not all at once.
    fn begin(Number { number }: Number) -> Result<Self, Fault> {
        let place = number & u64::from(u32::MAX);
        let left = match number >> 32 {
            0 => place % WORLD_LIFE + 1,
            lived => lived,
        };
        let at = place as f32;

        Ok(Self { place, left, position: at, velocity: 1.0 - at / WORLD_BODIES as f32, pushed: 0.0 })
    }

    #[tell]
    fn frame(&mut self, Frame { left, right }: Frame, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

    #[tell]
    fn nudge(&mut self, Nudge { push }: Nudge) -> Result<(), Fault> {
        self.pushed += push;

        Ok(())
    }

    #[tell]
    fn over(&mut self, _: Over, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, Speaks<(Ready,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

        Ok(())
    }
}

/// A question a keeper answers in the turn that handles it.
#[derive(Plain)]
#[kind(name = "aether.savina.question")]
struct Question {
    value: u64,
}

/// A question a keeper holds: it asks the stamper for a stamp, and answers what it held once the stamp comes back.
#[derive(Plain)]
#[kind(name = "aether.savina.fetch")]
struct Fetch {
    value: u64,
}

/// The answer to a [`Question`] or a [`Fetch`]: how many the keeper has answered.
#[derive(Plain)]
#[kind(name = "aether.savina.reply")]
struct Reply {
    answered: u64,
}

/// What a keeper asks the stamper for each fetch it holds.
#[derive(Plain)]
#[kind(name = "aether.savina.stamp")]
struct Stamp {
    value: u64,
}

/// The stamper's answer to a [`Stamp`].
#[derive(Plain)]
#[kind(name = "aether.savina.stamped")]
struct Stamped {
    value: u64,
}

/// What an asker keeps for each question it waits on: nothing.
#[derive(Plain)]
#[kind(name = "aether.savina.pending")]
struct Pending;

/// What a keeper keeps for each stamp it waits on: the fetch it holds.
#[derive(Bound)]
#[kind(name = "aether.savina.stamping")]
struct Stamping {
    held: Held<Reply>,
}

/// Why a request program went wrong: an answer did not come.
#[derive(Plain)]
#[kind(name = "aether.savina.no_answer")]
struct NoAnswer;

/// The fault of an answer that did not come.
fn no_answer() -> Fault {
    Fault::new(&NoAnswer).unwrap_or_else(|_| unreachable!("a unit encodes"))
}

/// What the root of a request program is made from: how many keepers answer, and whether they hold what they are
/// asked.
#[derive(Plain)]
#[kind(name = "aether.savina.questioning")]
struct Questioning {
    keepers: u64,
    holds: bool,
}

/// The root of a request program: it spawns the stamper, the keepers, and the askers, and tells each asker to start
/// as it begins.
#[derive(Plain)]
#[kind(name = "aether.savina.questions")]
struct Questions;

#[actor(key = u64, spawns(Stamper, Keeper, Asker))]
impl Questions {
    fn begin(questioning: Questioning, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Ok(stamper) = turn.spawn::<Stamper>(&0, Number { number: 0 }) else {
            panic!("the stamper begins");
        };
        let keepers = (0..questioning.keepers).map(|number| turn.spawn::<Keeper>(&number, Keeping { stamper }));
        let keepers = keepers.collect::<Result<Vec<_>, _>>().unwrap_or_else(|_| panic!("a keeper begins"));
        for number in 0..ASKERS {
            let asking = Asking { keepers: keepers.clone(), holds: questioning.holds };
            let Ok(asker) = turn.spawn::<Asker>(&number, asking) else {
                panic!("an asker begins");
            };
            turn.to(asker).send(&Start)?;
        }

        Ok(Self)
    }
}

/// The stamper of a request program: it answers each stamp in the turn that handles it.
#[derive(Plain)]
#[kind(name = "aether.savina.stamper")]
struct Stamper;

#[actor(key = u64, parent = Questions)]
impl Stamper {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[request]
    fn stamp(&mut self, Stamp { value }: Stamp) -> Result<Stamped, Fault> {
        Ok(Stamped { value: value.rotate_left(1) })
    }
}

/// What a keeper is made from: the stamper it asks for a stamp for each fetch it holds.
#[derive(Bound)]
#[kind(name = "aether.savina.keeping")]
struct Keeping {
    stamper: ActorRef<Stamper>,
}

/// A keeper of a request program: it answers a question at once, and holds a fetch until the stamp it asks for it
/// comes back, so it holds as many fetches as are asked of it at once. It counts what it answers.
#[derive(Bound)]
#[kind(name = "aether.savina.keeper")]
struct Keeper {
    stamper: ActorRef<Stamper>,
    answered: u64,
}

#[actor(key = u64, parent = Questions)]
impl Keeper {
    fn begin(keeping: Keeping) -> Result<Self, Fault> {
        Ok(Self { stamper: keeping.stamper, answered: 0 })
    }

    #[request]
    fn question(&mut self, _: Question) -> Result<Reply, Fault> {
        self.answered += 1;

        Ok(Reply { answered: self.answered })
    }

    #[request]
    fn fetch(&mut self, Fetch { value }: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Reply>, Fault> {
        let (held, later) = turn.hold();
        turn.to(self.stamper).ask(&Stamp { value }, &Stamping { held })?;

        Ok(later)
    }

    #[answer]
    fn stamped(
        &mut self,
        (answered, stamping): (Answered<Stamped>, Stamping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            return Err(no_answer());
        };
        self.answered += 1;
        turn.answer(stamping.held, &Reply { answered: self.answered })?;

        Ok(())
    }
}

/// What an asker is made from: the keepers it asks, and whether it asks what they hold.
#[derive(Bound)]
#[kind(name = "aether.savina.asking")]
struct Asking {
    keepers: Vec<ActorRef<Keeper>>,
    holds: bool,
}

/// An asker of a request program: it asks a batch of questions at once, spread over the keepers, and the next batch
/// once all are answered, until it has asked all it asks.
#[derive(Bound)]
#[kind(name = "aether.savina.asker")]
struct Asker {
    keepers: Vec<ActorRef<Keeper>>,
    holds: bool,
    asked: u64,
    answered: u64,
}

#[actor(key = u64, parent = Questions)]
impl Asker {
    fn begin(asking: Asking) -> Result<Self, Fault> {
        Ok(Self { keepers: asking.keepers, holds: asking.holds, asked: 0, answered: 0 })
    }

    #[tell]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.ask_batch(turn)
    }

    #[answer]
    fn reply(&mut self, (answered, _): (Answered<Reply>, Pending), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            return Err(no_answer());
        };
        self.answered += 1;
        if self.answered == ASKED {
            QUESTIONS_ANSWERED.fetch_add(1, Ordering::Relaxed);
        }

        let is_batch_done = self.answered == self.asked;
        let is_more = self.asked < ASKED;
        if is_batch_done && is_more {
            self.ask_batch(turn)?;
        }

        Ok(())
    }
}

impl Asker {
    /// Asks the next batch of questions, each of the next keeper in turn.
    fn ask_batch(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let end = (self.asked + ASKED_AT_ONCE).min(ASKED);
        let count = u64::try_from(self.keepers.len()).unwrap_or(1).max(1);
        for value in self.asked..end {
            let picked = usize::try_from(value % count).unwrap_or(0);
            let keeper = self.keepers[picked];
            if self.holds {
                turn.to(keeper).ask(&Fetch { value }, &Pending)?;
            } else {
                turn.to(keeper).ask(&Question { value }, &Pending)?;
            }
        }
        self.asked = end;

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
fn ready<Me, T>(turn: &mut Beginning<'_, Me>, children: &[ActorRef<T>]) -> Result<(), Fault>
where
    T: Handles<Ready>,
    T::From: Admits<T, Me, ()>,
{
    if let Some(last) = children.last() {
        turn.to(*last).send(&Ready)?;
    }

    Ok(())
}

/// A child of type `A` numbered `number`, spawned by `turn`.
fn spawned<Me, A, I>(turn: &mut Beginning<'_, Me>, number: u64) -> ActorRef<A>
where
    Me: Actor,
    A: ChildOf + Actor<Config = Number, Key = u64>,
    A::Parent: PlacesBeneath<Me, A>,
    Me::Spawns: ListsChild<Me, A, I>,
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

/// The roots of a request program, its answering split among `KEEPERS` keepers, which hold what they are asked when
/// `HOLDS`.
fn questions<const KEEPERS: u64, const HOLDS: bool>(engine: &mut Engine, round: u64) {
    let Ok(_) = engine.add_root::<Questions>(&round, Questioning { keepers: KEEPERS, holds: HOLDS }) else {
        panic!("the root begins");
    };
}

/// How many askers of a request program have had every question answered since its last run was checked.
static QUESTIONS_ANSWERED: AtomicU64 = AtomicU64::new(0);

/// Stops the benchmark unless every asker of a request program's last run had every question answered: an asker
/// waiting on an answer that was lost goes idle unseen, and the run looks quicker.
fn questions_check() {
    let answered = QUESTIONS_ANSWERED.swap(0, Ordering::Relaxed);
    assert_eq!(answered, ASKERS, "every asker has every question answered");
}

/// A request program's size: the root, the stamper, `KEEPERS` keepers, and the askers; each asker's start, and each
/// question and its answer, with a stamp and its answer beside each that is held when `HOLDS`.
const fn questions_size<const KEEPERS: u64, const HOLDS: bool>() -> Size {
    let each = if HOLDS {
        4
    } else {
        2
    };

    Size { actors: 2 + KEEPERS + ASKERS, mails: ASKERS * (1 + each * ASKED) }
}

/// The wide churn program's size: each root's share of the churn program, and the roots.
fn churn_wide_size() -> Size {
    let lived = CHURN_TOTAL / CHURN_SPAWNERS * CHURN_SPAWNERS;

    Size { actors: lived + CHURN_SPAWNERS, mails: lived * (CHURN_LIFE + 2) + 2 * CHURN_SPAWNERS }
}

const PROGRAMS: [Program; 14] = [
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
    Program { name: "ask-one", size: questions_size::<1, false>, start: questions::<1, false>, check: questions_check },
    Program {
        name: "ask-children",
        size: questions_size::<KEEPERS, false>,
        start: questions::<KEEPERS, false>,
        check: questions_check,
    },
    Program { name: "hold-one", size: questions_size::<1, true>, start: questions::<1, true>, check: questions_check },
    Program {
        name: "hold-children",
        size: questions_size::<KEEPERS, true>,
        start: questions::<KEEPERS, true>,
        check: questions_check,
    },
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
