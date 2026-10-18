//! How long a piece of mail between a native actor and a guest takes: a pinger and the echo guest volley, steady,
//! with fusion on (ADR-0015). A spike's measure.

#![allow(clippy::print_stdout)] // A measure prints its rounds.
#![allow(clippy::cast_precision_loss)] // Nanoseconds a piece, printed whole, lose nothing they show.

use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, ToAny, ToSender, ToWithoutMail, Turn};
use aethereal_chassis_headless::Monotonic;
use aethereal_chassis_headless::guest::{Guests, add_root};
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_substrate::{Clock, Engine};
use std::env;
use std::num::NonZeroUsize;

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
struct Volley {
    pings: u64,
}

impl Kind for Volley {
    const NAME: KindName = KindName::new("aether.bench.volley");
}

#[derive(Bound)]
struct Pinger;

impl Kind for Pinger {
    const NAME: KindName = KindName::new("aether.bench.pinger");
}

impl Actor for Pinger {
    type Config = Volley;
    type Key = u64;
    type Handles = (Pong,);
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
        if let Some(echo) = turn.resolve(&echo) {
            turn.to_any::<(Ping,)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Pinger {
    type Sends = (ToSender<Ping>,);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        if pong.n > 0 {
            turn.reply(&Ping { n: pong.n - 1 })?;
        }

        Ok(())
    }
}

fn main() {
    let profiled = env::args().any(|arg| arg == "profiled");
    let guests = if profiled {
        Guests::profiled()
    } else {
        Guests::default()
    };
    let Ok(code) = guests.code(ECHO) else {
        panic!("the echo guest is fit to run");
    };
    let Some(parts) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let clock = Monotonic::new();
    // `pingers=N`: N pingers volley with the one echo at once, so the echo is handed many items in each step.
    let pingers: u64 =
        env::args().find_map(|arg| arg.strip_prefix("pingers=").and_then(|count| count.parse().ok())).unwrap_or(1);
    let pings = 200_000 / pingers;
    for round in 0..5 {
        let instance = guests.instance(&code);
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64);
        let Ok(_) = add_root(&mut engine, &code, &instance, 0, &[]) else {
            panic!("the echo begins");
        };
        for key in 0..pingers {
            let Ok(_) = engine.add_root::<Pinger>(&key, Volley { pings }) else {
                panic!("a pinger begins");
            };
        }
        let started = clock.nanos();
        let mut steps = 0;
        while !engine.is_idle() {
            engine.step(&clock);
            steps += 1;
        }
        let pieces = 2 * (pings + 1) * pingers;
        let nanos = (clock.nanos() - started) as f64 / pieces as f64;
        println!("round {round}: {nanos:.0} ns a piece over {pieces} pieces, {steps} steps");
    }
}
