//! The chassis's tests: workers leave what one thread leaves.

use super::*;
use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, Level, Seated, Time, To, ToType, ToWithoutMail, Turn};
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
    type Handles = (Count,);
    type BeginSends = (ToType<Self, Count>,);
    type EndSends = ();

    fn begin(config: FanConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
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

    fn end(self, _turn: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Count> for Fan {
    type Sends = (ToType<Self, Count>,);

    fn handle(&mut self, Count { left }: Count, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
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
            let me = turn.me();
            turn.send(To::Held(me), &Count { left: left - 1 })?;
        }

        Ok(())
    }
}

/// How many children the root spawns.
const CHILDREN: u64 = 40;

/// An engine with the root fan, whose parts each run at most `hops` fused hops a step.
fn engine(hops: usize) -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops);
    let Ok(root) = engine.add_root::<Fan>(&0, FanConfig { number: 0, children: CHILDREN }) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Every fan in `engine` beneath and including `root`, in its seat, while all are live.
fn fans(engine: &Engine, root: ActorId) -> Vec<Seated> {
    let children = (1..=CHILDREN).map(|number| child_id(root, &PathSegment::new(Fan::NAME, &number)));

    iter::once(root).chain(children).filter_map(|id| Some(Seated { id, seat: engine.table().seat(id)? })).collect()
}

/// Everything the engine shows of each of `fans`: whether it is live, and its log.
fn shown(engine: &Engine, fans: &[Seated]) -> Vec<String> {
    let shown =
        |fan: &Seated| format!("{:?} {:?}", engine.table().is_live(fan.id), engine.log(*fan).collect::<Vec<_>>());

    fans.iter().map(shown).collect()
}

#[test]
fn workers_leave_what_one_thread_leaves() {
    // Catches a share whose actors are not put back, turns applied in the order workers finished them, a lend that
    // returns before every worker has answered, and chains fused differently by how many workers ran them.
    for hops in [0, 8] {
        let (mut alone, root) = engine(hops);
        let alone_fans = fans(&alone, root);
        assert_eq!(alone_fans.len(), 1 + usize::try_from(CHILDREN).unwrap_or(usize::MAX));
        let clock = Monotonic::new();
        while !alone.is_idle() {
            alone.step(Time::ZERO, &clock);
        }

        let Some(four) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let (engine, _) = engine(hops);
        let split_fans = fans(&engine, root);
        let mut headless = Headless::new(engine, four);
        headless.run_until_idle();

        assert_eq!(headless.engine().step_number(), alone.step_number());
        assert_eq!(shown(headless.engine(), &split_fans), shown(&alone, &alone_fans));
    }
}

#[test]
fn a_worker_the_plan_keeps_awake_is_not_parked_by_a_step_split_among_fewer() {
    // Catches the pool parking every worker past a split step's shares whatever the plan kept awake, so a step split
    // among fewer workers than are running parks the rest, and the step after wakes them again.
    let Some(four) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    // One actor in each part, all of one kind, so every part is expected to cost the same and a step is split among
    // every worker it may be.
    let mut engine = engine(0).0.rehearsal();
    let clock = Monotonic::new();
    let mut pool = Pool::new(four, &clock);
    let all = Workers { active: 4, max: 4, wake_nanos: 0, split: SplitCost::NONE };
    let two = Workers { active: 2, max: 2, ..all };
    step(&mut engine, &mut pool, clock, all);
    let spread = step(&mut engine, &mut pool, clock, all);
    assert_eq!(spread.sharing, 4);

    let parks = pool.measured().parks;
    let fewer = step(&mut engine, &mut pool, clock, two);
    assert_eq!(fewer.sharing, 2);
    assert_eq!(pool.measured().parks, parks);
}
