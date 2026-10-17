//! How long a piece of mail between a native actor and a guest takes: a pinger and the echo guest volley, steady,
//! with fusion on (ADR-0015). A spike's measure.

use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, ToAny, ToSender, ToWithoutMail, Turn};
use aethereal_chassis_headless::Monotonic;
use aethereal_chassis_headless::guest::{Guests, add_root};
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_substrate::Engine;
use std::num::NonZeroUsize;
use std::time::Instant;

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
    type Key = ();
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
    let profiled = std::env::args().any(|arg| arg == "profiled");
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
    let pings = 200_000;
    for round in 0..5 {
        let instance = guests.instance(&code);
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64);
        let (Ok(_), Ok(_)) =
            (add_root(&mut engine, &code, &instance, 0, &[]), engine.add_root::<Pinger>(&(), Volley { pings }))
        else {
            panic!("the roots begin");
        };
        let started = Instant::now();
        let mut steps = 0;
        while !engine.is_idle() {
            engine.step(&clock);
            steps += 1;
        }
        let pieces = 2 * (pings + 1);
        let nanos = started.elapsed().as_nanos() as f64 / pieces as f64;
        println!("round {round}: {nanos:.0} ns a piece over {pieces} pieces, {steps} steps");
    }
}
