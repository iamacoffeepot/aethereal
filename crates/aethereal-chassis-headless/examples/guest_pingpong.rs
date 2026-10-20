//! How long a piece of mail between a native actor and a guest takes: a pinger and the echo guest volley, steady,
//! with fusion on (ADR-0015). A spike's measure.

#![allow(clippy::print_stdout)] // A measure prints its rounds.
#![allow(clippy::cast_precision_loss)] // Nanoseconds a piece, printed whole, lose nothing they show.

use aethereal_actor::{
    Actor, Beginning, ChildOf, Ending, Fault, Handles, Time, ToAny, ToSender, ToWithoutMail, Top, Turn,
};
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

impl ChildOf for Pinger {
    type Parent = Top;
}

impl Actor for Pinger {
    type Config = Volley;
    type Key = u64;
    type Handles = (Pong,);
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(volley: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
        if let Some(echo) = turn.resolve(&echo) {
            turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: volley.pings })?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Pinger {
    type Sends = (ToSender<Ping>,);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if pong.n > 0 {
            turn.reply(&Ping { n: pong.n - 1 })?;
        }

        Ok(())
    }
}

fn main() {
    let profiled = env::args().any(|arg| arg == "profiled");
    let runtime = if profiled {
        Runtime::profiled()
    } else {
        Runtime::default()
    };
    let runtime = Arc::new(runtime);
    let Some(parts) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let clock = Monotonic::new();
    // `pingers=N`: N pingers volley with the one echo at once, so the echo is handed many items in each step.
    let pingers: u64 =
        env::args().find_map(|arg| arg.strip_prefix("pingers=").and_then(|count| count.parse().ok())).unwrap_or(1);
    let pings = 200_000 / pingers;
    // `rounds=N`: how many rounds to run, five by default.
    let rounds: u64 =
        env::args().find_map(|arg| arg.strip_prefix("rounds=").and_then(|count| count.parse().ok())).unwrap_or(5);
    for round in 0..rounds {
        let mut setup = Engine::new(parts, 1024, 0, 1 << 20, 64).with_guests(runtime.clone());
        let Ok(code) = setup.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };
        let pingers_made = (0..pingers).map(|key| (key, Volley { pings }));
        let made = setup
            .add_root_from::<guest::Echo>(&code, &(), &guest::EchoConfig)
            .and_then(|setup| setup.add_roots::<Pinger>(pingers_made));
        let Ok(setup) = made else {
            panic!("the echo and the pingers begin");
        };
        let mut engine = setup.ready();
        let started = clock.nanos();
        let mut steps = 0;
        while !engine.is_idle() {
            engine.step(Time::from_nanos(clock.nanos()), &clock);
            steps += 1;
        }
        let pieces = 2 * (pings + 1) * pingers;
        let nanos = (clock.nanos() - started) as f64 / pieces as f64;
        println!("round {round}: {nanos:.0} ns a piece over {pieces} pieces, {steps} steps");
    }
}
