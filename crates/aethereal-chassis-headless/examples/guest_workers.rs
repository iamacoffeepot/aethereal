//! How a blob's actors spread over several workers fare: pairs of a native pinger and a responder guest of one blob
//! volley, each pair to itself, on 16 parts run by the worker pool, steady, with fusion on (ADR-0015). Each part runs
//! its responders in its own instance of the blob. A spike's measure.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example guest_workers [pairs=N] [workers=N,...]
//! [hops=N] [rounds=N]`.

#![allow(clippy::print_stdout)] // A measure prints its rounds.
#![allow(clippy::cast_precision_loss)] // Nanoseconds a piece, printed whole, lose nothing they show.

use aethereal_actor::BlobRef;
use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, ToAny, ToSender, ToWithoutMail, Turn};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_chassis_headless::{Headless, Monotonic};
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use aethereal_guest_echo as guest;
use aethereal_substrate::{Clock, Engine};
use std::env;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;

const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

/// The pieces of mail every round sends, shared among its pairs.
const PIECES: u64 = 400_000;

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
struct Shut;

impl Kind for Shut {
    const NAME: KindName = KindName::new("aether.guest.shut");
}

/// Whom a pinger volleys with, and how many times.
#[derive(Plain)]
struct Volley {
    with: u64,
    pings: u64,
}

impl Kind for Volley {
    const NAME: KindName = KindName::new("aether.bench.volley");
}

/// The native side of a pair: it pings its responder until its volley is spent, then tells it to close, and closes.
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
        let responder = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.responder"), &volley.with));
        if let Some(responder) = turn.resolve(&responder) {
            turn.to_any::<(Ping,)>(ToWithoutMail::Held(responder)).send(&Ping { n: volley.pings })?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Pinger {
    type Sends = (ToSender<Ping>, ToSender<Shut>);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        if pong.n > 0 {
            turn.reply(&Ping { n: pong.n - 1 })?;
        } else {
            turn.reply(&Shut)?;
            turn.close();
        }

        Ok(())
    }
}

/// The responder written natively, for the measure's `native` run: what the engine does with the same volleys when
/// no guest runs them.
#[derive(Bound)]
struct NativeResponder {
    answered: u64,
}

impl Kind for NativeResponder {
    const NAME: KindName = KindName::new("aether.guest.responder");
}

impl Actor for NativeResponder {
    type Config = Volley;
    type Key = u64;
    type Handles = (Ping, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Volley, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { answered: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for NativeResponder {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        self.answered += 1;
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Shut> for NativeResponder {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// Which responder the pairs volley with.
#[derive(Clone, Copy)]
enum Responding {
    Guest,
    Native,
}

/// One round's pairs, keyed from `first`: every responder, then every pinger, so each pinger finds its responder as it
/// begins. A root sits in the part with fewest actors, so with no more pairs than parts the `k`th pinger sits beside
/// the `k`th responder, and their volley fuses.
fn start(engine: &mut Engine, code: &BlobRef, responding: Responding, first: u64, pairs: u64) {
    let pings = PIECES / 2 / pairs;
    for key in first..first + pairs {
        let began = match responding {
            Responding::Guest => engine.add_root_from::<guest::Responder>(code, &key, &guest::EchoConfig),
            Responding::Native => engine.add_root::<NativeResponder>(&key, Volley { with: key, pings }),
        };
        let Ok(_) = began else {
            panic!("a responder begins");
        };
    }
    for key in first..first + pairs {
        let Ok(_) = engine.add_root::<Pinger>(&key, Volley { with: key, pings }) else {
            panic!("a pinger begins");
        };
    }
}

fn arg<T: FromStr>(name: &str) -> Option<T> {
    env::args().find_map(|arg| arg.strip_prefix(name).and_then(|given| given.parse().ok()))
}

fn main() {
    let runtime = Arc::new(Runtime::default());
    let pairs: u64 = arg("pairs=").unwrap_or(16);
    let is_native = env::args().any(|arg| arg == "native");
    let responding = if is_native {
        Responding::Native
    } else {
        Responding::Guest
    };
    let hops: usize = arg("hops=").unwrap_or(64);
    let rounds: u64 = arg("rounds=").unwrap_or(20);
    let workers: Vec<usize> = env::args()
        .find_map(|arg| arg.strip_prefix("workers=").map(str::to_owned))
        .map_or_else(|| vec![1, 4, 16], |given| given.split(',').filter_map(|count| count.parse().ok()).collect());
    let pieces = 2 * (PIECES / 2 / pairs + 1) * pairs;

    println!("pairs workers  steady ns   best ns");
    for count in workers {
        let workers = NonZeroUsize::MIN.saturating_add(count.max(1) - 1);
        let parts = NonZeroUsize::MIN.saturating_add(15);
        let mut engine = Engine::new(parts, 1024, 1 << 30, 1 << 30, hops).with_guests(runtime.clone());
        let Ok(code) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };
        let mut headless = Headless::new(engine, workers);
        let mut took: Vec<u64> = (0..rounds)
            .map(|round| {
                let clock = Monotonic::new();
                start(headless.engine_mut(), &code, responding, round * pairs, pairs);
                headless.run_until_idle();

                clock.nanos()
            })
            .collect();
        let mut settled = took.split_off((took.len() / 3).max(1).min(took.len().saturating_sub(1)));
        settled.sort_unstable();
        let median = settled.get(settled.len() / 2).copied().unwrap_or_default();
        let best = settled.first().copied().unwrap_or_default();
        let per = |nanos: u64| nanos as f64 / pieces as f64;
        println!("{pairs:5} {count:7} {:10.0} {:9.0}", per(median), per(best));
        let measured = headless.measured();
        let steps = measured.steps.max(1);
        println!(
            "      steps {} plan {} step {} apply {} idle {} (ns a step) wakes {} hopped {:?}",
            measured.steps,
            measured.plan_nanos / steps,
            measured.step_nanos / steps,
            measured.apply_nanos / steps,
            measured.idle_nanos / steps,
            measured.wakes,
            headless.engine().hopped(),
        );
    }
}
