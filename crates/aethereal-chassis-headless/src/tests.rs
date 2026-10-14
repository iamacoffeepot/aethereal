//! The chassis's tests: workers leave what one thread leaves.

use super::*;
use aethereal_actor::{Actor, Beginning, Ending, Fault, Item, Level, To, ToWithoutMail, Turn};
use aethereal_data::{ActorId, Kind, KindName, PathSegment, Plain, TypedKey};
use aethereal_substrate::child_id;
use std::iter;

/// How many turns a fan has left to take.
#[derive(Plain)]
struct Count {
    left: u64,
}

impl Kind for Count {
    const NAME: KindName = KindName::new("aether.test.count");
}

/// A fan's number, and how many children it spawns.
#[derive(Plain)]
struct FanConfig {
    number: u64,
    children: u64,
}

impl Kind for FanConfig {
    const NAME: KindName = KindName::new("aether.test.fan_config");
}

/// An actor that, as the root, spawns its children and counts each down; as a child, works an amount that grows
/// with its number, logs where it is, tells its parent, and counts itself down until it closes.
#[derive(Plain)]
struct Fan {
    number: u64,
}

impl Kind for Fan {
    const NAME: KindName = KindName::new("aether.test.fan");
}

/// How many turns each child takes.
const TURNS: u64 = 6;

impl Actor for Fan {
    type Config = FanConfig;
    type Key = u64;

    fn begin(config: FanConfig, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        let is_root = config.number == 0;
        if is_root {
            for number in 1..=config.children {
                let config = FanConfig { number, children: 0 };
                if turn.spawn::<Self>(&number, config).is_ok() {
                    turn.send(ToWithoutMail::ChildInstanced(Self::NAME, number.to_name()), &Count { left: TURNS })?;
                }
            }
        }

        Ok(Self { number: config.number })
    }

    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        let Some(Count { left }) = mail.read()? else {
            return Ok(());
        };
        let is_root = self.number == 0;
        if is_root {
            turn.log(Level::Info, format!("heard {left}"));

            return Ok(());
        }

        let worked = (0..self.number * 1_000).fold(self.number, |sum, step| sum.wrapping_mul(31).wrapping_add(step));
        turn.log(Level::Info, format!("{} at {left} worked {worked}", self.number));
        turn.send(To::Parent, &Count { left })?;
        if left == 0 {
            turn.close();
        } else {
            turn.send(To::Held(turn.me()), &Count { left: left - 1 })?;
        }

        Ok(())
    }

    fn end(self, _turn: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// How many children the root spawns.
const CHILDREN: u64 = 40;

/// An engine with the root fan.
fn engine() -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024);
    let Ok(root) = engine.add_root::<Fan>(&0, FanConfig { number: 0, children: CHILDREN }) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Everything the engine shows of each fan: its standing and its log.
fn shown(engine: &Engine, root: ActorId) -> Vec<String> {
    let children = (1..=CHILDREN).map(|number| child_id(root, &PathSegment::new(Fan::NAME, &number)));

    iter::once(root)
        .chain(children)
        .map(|id| format!("{:?} {:?}", engine.table().standing(id), engine.log(id).collect::<Vec<_>>()))
        .collect()
}

#[test]
fn workers_leave_what_one_thread_leaves() {
    // Catches a share whose actors are not put back, turns applied in the order workers finished them, and a lend
    // that returns before every worker has answered.
    let (mut alone, root) = engine();
    let clock = Monotonic::new();
    while !alone.is_idle() {
        alone.step(&clock);
    }

    let Some(four) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let (engine, _) = engine();
    let mut headless = Headless::new(engine, four);
    headless.run_until_idle();

    assert_eq!(headless.engine().step_number(), alone.step_number());
    assert_eq!(shown(headless.engine(), root), shown(&alone, root));
}
