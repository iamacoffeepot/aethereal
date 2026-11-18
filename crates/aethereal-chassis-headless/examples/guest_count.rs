//! How long a piece of mail a guest is handed among many waiting takes: a counter sends the echo guest bursts of
//! increments, each closed by a ping it waits on the pong of, steady, with fusion on (ADR-0015). `native` runs a native
//! echo in its place. A spike's measure.

#![allow(clippy::print_stdout)] // A measure prints its rounds.
#![allow(clippy::cast_precision_loss)] // Nanoseconds a piece, printed whole, lose nothing they show.

use aethereal_actor::{Answered, Beginning, Fault, ProtocolRef, Time, Turn, actor};
use aethereal_chassis_headless::Monotonic;
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{ActorPath, Bound, KindName, PathSegment, Plain};
use aethereal_guest_echo as guest;
use aethereal_guest_echo::{Increment, Ping, Pings, Pong};
use aethereal_substrate::{Clock, Engine};
use std::env;
use std::num::NonZeroUsize;
use std::sync::Arc;

const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

#[derive(Plain)]
#[kind(name = "aether.bench.bursts")]
struct Bursts {
    bursts: u64,
    each: u64,
}

/// What a counter keeps for each ping until it is answered: nothing, as the pong says how many bursts are left.
#[derive(Plain)]
#[kind(name = "aether.bench.burst")]
struct Burst;

/// A native actor that asks the echo a ping, and on each pong while bursts are left tells it a burst of increments and
/// asks it the next ping.
#[derive(Bound)]
#[kind(name = "aether.bench.counter")]
struct Counter {
    each: u64,
    pings: ProtocolRef<Pings>,
    counts: ProtocolRef<(Increment,)>,
}

#[actor]
impl Counter {
    fn begin(bursts: Bursts, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
        let Some(echo) = turn.resolve(&echo) else {
            panic!("the echo begins before its counter");
        };
        let (Some(pings), Some(counts)) = (turn.cast::<Pings>(echo), turn.cast::<(Increment,)>(echo)) else {
            panic!("the echo takes pings and increments");
        };
        turn.to(pings).ask(&Ping { n: bursts.bursts }, &Burst)?;

        Ok(Self { each: bursts.each, pings, counts })
    }

    #[answer]
    fn pong(&mut self, (pong, Burst): (Answered<Pong>, Burst), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Answered::Yes(pong) = pong else {
            return Ok(());
        };
        if pong.n > 0 {
            for _ in 0..self.each {
                turn.to(self.counts).send(&Increment)?;
            }
            turn.to(self.pings).ask(&Ping { n: pong.n - 1 }, &Burst)?;
        }

        Ok(())
    }
}

/// The echo, native: what the guest does, run in the engine's own code.
#[derive(Bound)]
#[kind(name = "aether.guest.echo")]
struct Echo {
    counted: u64,
}

#[derive(Plain)]
#[kind(name = "aether.guest.echo_config")]
struct EchoConfig;

#[actor]
impl Echo {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self { counted: 0 })
    }

    #[request]
    fn ping(&mut self, ping: Ping) -> Result<Pong, Fault> {
        Ok(Pong { n: ping.n })
    }

    #[tell]
    fn increment(&mut self, _: Increment) -> Result<(), Fault> {
        self.counted += 1;

        Ok(())
    }
}

fn main() {
    let native = env::args().any(|arg| arg == "native");
    let runtime = Arc::new(Runtime::default());
    let Some(parts) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let clock = Monotonic::new();
    // `each=N`: the increments in each burst.
    let each: u64 =
        env::args().find_map(|arg| arg.strip_prefix("each=").and_then(|count| count.parse().ok())).unwrap_or(1000);
    let bursts = 1_000_000 / each;
    // `rounds=N`: how many rounds to run, five by default.
    let rounds: u64 =
        env::args().find_map(|arg| arg.strip_prefix("rounds=").and_then(|count| count.parse().ok())).unwrap_or(5);
    for round in 0..rounds {
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64).with_guests(runtime.clone()).ready();
        let Ok(code) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };
        if native {
            let Ok(_) = engine.add_root::<Echo>(&(), EchoConfig) else {
                panic!("the echo begins");
            };
        } else {
            let Ok(_) = engine.add_root_from::<guest::Echo>(&code, &(), &guest::EchoConfig) else {
                panic!("the echo begins");
            };
        }
        let Ok(_) = engine.add_root::<Counter>(&(), Bursts { bursts, each }) else {
            panic!("the counter begins");
        };
        let started = clock.nanos();
        let mut steps = 0;
        while !engine.is_idle() {
            engine.step(Time::from_nanos(clock.nanos()), &clock);
            steps += 1;
        }
        let pieces = bursts * (each + 2);
        let nanos = (clock.nanos() - started) as f64 / pieces as f64;
        println!("round {round}: {nanos:.0} ns a piece over {pieces} pieces, {steps} steps");
    }
}
