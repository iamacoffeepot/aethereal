//! How long a piece of mail a guest is handed among many waiting takes: a counter sends the echo guest bursts of
//! increments, each closed by a ping it waits on the pong of, steady, with fusion on (ADR-0015). `native` runs a native
//! echo in its place. A spike's measure.

#![allow(clippy::print_stdout)] // A measure prints its rounds.
#![allow(clippy::cast_precision_loss)] // Nanoseconds a piece, printed whole, lose nothing they show.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Time, ToAny, ToSender, Top, Turn};
use aethereal_chassis_headless::Monotonic;
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_guest_echo as guest;
use aethereal_substrate::{Clock, Engine};
use std::env;
use std::num::NonZeroUsize;
use std::sync::Arc;

const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

#[derive(Plain)]
struct Ping {
    n: u64,
}

impl Kind for Ping {
    const NAME: KindName = KindName::new("aether.guest.ping");
}

#[derive(Plain)]
struct Pong {
    n: u64,
}

impl Kind for Pong {
    const NAME: KindName = KindName::new("aether.guest.pong");
}

#[derive(Plain)]
struct Increment;

impl Kind for Increment {
    const NAME: KindName = KindName::new("aether.guest.increment");
}

#[derive(Plain)]
struct Bursts {
    bursts: u64,
    each: u64,
}

impl Kind for Bursts {
    const NAME: KindName = KindName::new("aether.bench.bursts");
}

#[derive(Bound)]
struct Counter {
    each: u64,
}

impl Kind for Counter {
    const NAME: KindName = KindName::new("aether.bench.counter");
}

impl ChildOf for Counter {
    type Parent = Top;
}

impl Actor for Counter {
    type Config = Bursts;
    type Key = ();
    type Handles = (Pong,);
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(bursts: Bursts, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
        if let Some(echo) = turn.resolve(&echo).and_then(|found| turn.cast(found)) {
            turn.to(echo).send(&Ping { n: bursts.bursts })?;
        }

        Ok(Self { each: bursts.each })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Counter {
    type Sends = (ToSender<Increment>, ToSender<Ping>);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if pong.n > 0 {
            for _ in 0..self.each {
                turn.reply(&Increment)?;
            }
            turn.reply(&Ping { n: pong.n - 1 })?;
        }

        Ok(())
    }
}

/// The echo, native: what the guest does, run in the engine's own code.
#[derive(Bound)]
struct Echo {
    counted: u64,
}

impl Kind for Echo {
    const NAME: KindName = KindName::new("aether.guest.echo");
}

#[derive(Plain)]
struct EchoConfig;

impl Kind for EchoConfig {
    const NAME: KindName = KindName::new("aether.guest.echo_config");
}

impl ChildOf for Echo {
    type Parent = Top;
}

impl Actor for Echo {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Increment);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { counted: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Echo {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Increment> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Increment, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
