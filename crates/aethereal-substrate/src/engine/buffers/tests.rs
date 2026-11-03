//! The buffers of freed blobs, handed back to the actors that put them and handed out again by `turn.buffer`, and
//! the buffer a job that fills buffers is lent (ADR-0042).

use super::super::*;
use aethereal_actor::{
    Actor, Answer, Answered, ChildOf, Ending, Fills, Handles, Job, Path, Runner, Running, Services, Tells, ToType, Top,
    Turn, path,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeSet;

/// What a builder does in one of its turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Order {
    /// Takes a buffer with room for this many bytes, notes what it took, and drops it.
    Take(u64),
    /// Takes a buffer with room for this many bytes, writes three bytes into it, puts it, and keeps the blob.
    Put(u64),
    /// Puts as `Put` does, and lends the blob to the builder keyed `to`, keeping none of it.
    Lend { room: u64, to: u64 },
    /// Runs a [`Fill`] of a buffer of this many bytes, keeps the blob it answers with, and goes on once it is answered.
    Fill(u64),
    /// Runs a [`Draw`], a job that fills buffers, of a buffer with room for this many bytes, notes what it took, and
    /// goes on once it is answered.
    Draw(u64),
    /// Runs a [`Peek`], a job that fills nothing, as `Draw` runs a draw.
    Peek(u64),
    /// Runs a [`Draw`] of a buffer with room for this many bytes and cancels it in the same turn, before it is handed
    /// out.
    DrawCancelled(u64),
    /// Lets go of every blob it keeps.
    LetGo,
}

/// What a builder does in each of its turns to come, one list of orders a turn: it sends itself the rest after each.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.plan")]
struct Plan {
    turns: Vec<Vec<Order>>,
}

/// A blob lent by one builder to another.
#[derive(Bound)]
#[kind(name = "aether.test.lent")]
struct Lent {
    blob: Bytes,
}

/// A buffer a builder took: its capacity, and how many bytes were in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Took {
    capacity: u64,
    len: u64,
}

/// A job that puts three bytes in a buffer of `capacity` bytes it makes, and answers with the blob.
#[derive(Plain)]
#[kind(name = "aether.test.fill")]
struct Fill {
    capacity: u64,
}

/// What a fill answers with.
#[derive(Bound)]
#[kind(name = "aether.test.filled")]
struct Filled {
    blob: Bytes,
}

impl Job for Fill {
    type Done = Filled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Filled, Fault> {
        let mut buffer = Vec::with_capacity(size(self.capacity));
        buffer.extend_from_slice(&[7; 3]);

        Ok(Filled { blob: job.put(buffer) })
    }
}

/// A job that fills buffers: takes a buffer with room for `room` bytes and answers with what it took.
#[derive(Plain)]
#[kind(name = "aether.test.draw")]
struct Draw {
    room: u64,
}

/// A job that takes a buffer as a draw does, but whose type fills nothing.
#[derive(Plain)]
#[kind(name = "aether.test.peek")]
struct Peek {
    room: u64,
}

/// What a draw or a peek took: the capacity of its buffer.
#[derive(Plain)]
#[kind(name = "aether.test.drew")]
struct Drew {
    capacity: u64,
}

impl Job for Draw {
    type Done = Drew;
    const RUNNER: Runner = Runner::Compute;
    const FILLS: Fills = Fills::Buffers;

    async fn run(self, job: &mut Running) -> Result<Drew, Fault> {
        Ok(Drew { capacity: count(job.buffer(size(self.room)).capacity()) })
    }
}

impl Job for Peek {
    type Done = Drew;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Drew, Fault> {
        Ok(Drew { capacity: count(job.buffer(size(self.room)).capacity()) })
    }
}

/// An actor that takes buffers, puts and lends blobs, runs fills, and lets go of what it keeps, as its plan says, and
/// keeps what each buffer it took was like.
#[derive(Bound)]
#[kind(name = "aether.test.builder")]
struct Builder {
    held: Vec<Bytes>,
    took: Vec<Took>,
}

impl ChildOf for Builder {
    type Parent = Top;
}

impl Actor for Builder {
    type Config = Plan;
    type Key = u64;
    type Handles = (Plan, Lent, Answer<Filled, Plan>, Answer<Drew, Plan>);
    type BeginSends = (ToType<Self, Plan>,);
    type EndSends = ();

    fn begin(plan: Plan, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(turn.me()).send(&plan)?;

        Ok(Self { held: Vec::new(), took: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Plan> for Builder {
    type Sends = (ToType<Self, Plan>, Tells<Path<Self>, Lent>);

    fn handle(&mut self, mut plan: Plan, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if plan.turns.is_empty() {
            return Ok(());
        }
        let orders = plan.turns.remove(0);
        for order in orders {
            match order {
                Order::Take(room) => {
                    let buffer = turn.buffer(size(room));
                    self.took.push(Took { capacity: count(buffer.capacity()), len: count(buffer.len()) });
                }
                Order::Put(room) => {
                    let blob = put(turn, room)?;
                    self.held.push(blob);
                }
                Order::Lend { room, to } => {
                    let blob = put(turn, room)?;
                    turn.to(path::<Self>(to)).send(&Lent { blob })?;
                }
                Order::Fill(capacity) => {
                    turn.run(&Fill { capacity }, &plan)?;
                    return Ok(());
                }
                Order::Draw(room) => {
                    turn.run(&Draw { room }, &plan)?;
                    return Ok(());
                }
                Order::Peek(room) => {
                    turn.run(&Peek { room }, &plan)?;
                    return Ok(());
                }
                Order::DrawCancelled(room) => {
                    let job = turn.run(&Draw { room }, &plan)?;
                    turn.cancel(job);
                }
                Order::LetGo => self.held.clear(),
            }
        }
        turn.to(turn.me()).send(&plan)?;

        Ok(())
    }
}

impl Handles<Lent> for Builder {
    type Sends = ();

    fn handle(&mut self, lent: Lent, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.held.push(lent.blob);

        Ok(())
    }
}

impl Handles<Answer<Filled, Plan>> for Builder {
    type Sends = (ToType<Self, Plan>,);

    fn handle(
        &mut self,
        (filled, plan): (Answered<Filled>, Plan),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        if let Answered::Yes(Filled { blob }) = filled {
            self.held.push(blob);
        }
        turn.to(turn.me()).send(&plan)?;

        Ok(())
    }
}

impl Handles<Answer<Drew, Plan>> for Builder {
    type Sends = (ToType<Self, Plan>,);

    fn handle(
        &mut self,
        (drew, plan): (Answered<Drew>, Plan),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        if let Answered::Yes(Drew { capacity }) = drew {
            self.took.push(empty(capacity));
        }
        turn.to(turn.me()).send(&plan)?;

        Ok(())
    }
}

/// Takes a buffer with room for `room` bytes, writes three bytes into it, and puts it.
fn put<S>(turn: &mut Turn<'_, Builder, S>, room: u64) -> Result<Bytes, Fault> {
    let mut buffer = turn.buffer(size(room));
    buffer.extend_from_slice(&[7; 3]);

    turn.put(buffer).map_err(|_| Fault::of_parts(KindId::of::<Lent>(), Vec::new()))
}

/// `value` as a `usize`.
fn size(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// `value` as a `u64`.
fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// A buffer taken empty, of `capacity` bytes.
const fn empty(capacity: u64) -> Took {
    Took { capacity, len: 0 }
}

/// A clock that never moves.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine of four parts, its jobs run inline, as `setup` sets it up further.
fn engine(setup: impl FnOnce(Setup<()>) -> Setup<()>) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let jobs = Arc::new(Inline::new(Arc::new(Services::none())));

    setup(Engine::new(parts, 1024, 0, 1 << 20, 0).with_jobs(jobs)).ready()
}

/// Makes the builder keyed `key` in `engine`, to do as `turns` says, one list of orders a turn.
fn build(engine: &mut Engine, key: u64, turns: &[&[Order]]) -> Seated {
    let plan = Plan { turns: turns.iter().map(|orders| orders.to_vec()).collect() };
    let Ok(id) = engine.add_root::<Builder>(&key, plan) else {
        panic!("the builder begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the builder is seated");
    };

    Seated { id, seat }
}

/// Steps `engine`, bringing in each job that finished, until nothing is left to run.
fn settle(engine: &mut Engine) {
    engine.bring();
    while !engine.is_idle() {
        engine.step(Time::ZERO, &Still);
        engine.bring();
    }
}

/// What each buffer the builder `builder` took was like, in order.
fn took(engine: &Engine, builder: Seated) -> Vec<Took> {
    let Some(builder) = engine.state::<Builder>(builder) else {
        panic!("the builder is live");
    };

    builder.took.clone()
}

#[test]
fn a_freed_blobs_buffer_comes_back_emptied_to_the_actor_that_put_it() {
    // Catches a freed blob's buffer dropped rather than handed back, one handed back before the blob is freed, one
    // handed back with its bytes still in it, and one looked for in another part than its maker sits in.
    let mut engine = engine(|setup| setup);
    let turns: &[&[Order]] = &[&[Order::Put(4096)], &[Order::Take(3000)], &[Order::LetGo], &[Order::Take(3000)]];
    let builders: Vec<Seated> = (1..=4).map(|key| build(&mut engine, key, turns)).collect();
    settle(&mut engine);

    let parts: BTreeSet<usize> = builders.iter().map(|builder| builder.seat.index() % 4).collect();
    assert!(parts.len() > 1, "the builders sit in several parts");
    for builder in builders {
        assert_eq!(took(&engine, builder), [empty(3000), empty(4096)], "{builder:?}");
    }
}

#[test]
fn a_buffer_that_would_take_its_actor_past_what_it_keeps_is_freed() {
    // Catches buffers kept past `most_kept_bytes`: each builder frees two buffers of 4096 bytes at once, and one that
    // keeps at most 6000 bytes keeps only one of them, where one that keeps the default keeps both. Each take asks for
    // 3000 bytes, within twice of which a kept buffer of 4096 is handed out.
    let turns: &[&[Order]] =
        &[&[Order::Put(4096), Order::Put(4096)], &[Order::LetGo], &[Order::Take(3000), Order::Take(3000)]];
    let mut kept = engine(|setup| setup.with_most_kept_bytes(6000));
    let mut unbounded = engine(|setup| setup);
    let (once, both) = (build(&mut kept, 1, turns), build(&mut unbounded, 1, turns));
    settle(&mut kept);
    settle(&mut unbounded);

    assert_eq!(took(&kept, once), [empty(4096), empty(3000)]);
    assert_eq!(took(&unbounded, both), [empty(4096), empty(4096)]);
}

#[test]
fn a_jobs_blob_goes_back_to_the_actor_that_ran_the_job() {
    // Catches the blobs a job put taken in as the chassis's, whose buffers are dropped when they are freed, rather than
    // as the actor's that ran the job.
    let mut engine = engine(|setup| setup);
    let wait: &[Order] = &[];
    let turns: &[&[Order]] = &[&[Order::Fill(4096)], &[Order::LetGo], wait, wait, wait, &[Order::Take(3000)]];
    let builder = build(&mut engine, 1, turns);
    settle(&mut engine);

    assert_eq!(took(&engine, builder), [empty(4096)]);
}

#[test]
fn a_buffer_goes_back_to_the_actor_that_put_it_and_not_to_the_last_to_hold_it() {
    // Catches a freed blob's buffer handed to the actor that let go of its last reference, which never put it.
    let mut engine = engine(|setup| setup);
    let wait: &[Order] = &[];
    let lender: &[&[Order]] =
        &[&[Order::Lend { room: 4096, to: 2 }], wait, wait, wait, wait, wait, &[Order::Take(3000)]];
    let borrower: &[&[Order]] = &[wait, wait, &[Order::LetGo], wait, wait, wait, &[Order::Take(3000)]];
    let lender = build(&mut engine, 1, lender);
    let borrower = build(&mut engine, 2, borrower);
    settle(&mut engine);

    assert_eq!(took(&engine, lender), [empty(4096)]);
    assert_eq!(took(&engine, borrower), [empty(3000)]);
}

#[test]
fn a_job_that_fills_buffers_is_lent_its_actors_largest_kept_buffer() {
    // Catches a job that fills buffers lent nothing, which then makes its own, and one lent the buffer that fits its
    // room best rather than the largest, which takes from the actor the buffer its own next take would have had.
    let mut engine = engine(|setup| setup);
    let turns: &[&[Order]] =
        &[&[Order::Put(4096), Order::Put(8192)], &[Order::LetGo], &[Order::Draw(4096)], &[Order::Take(4096)]];
    let builder = build(&mut engine, 1, turns);
    settle(&mut engine);

    assert_eq!(took(&engine, builder), [empty(8192), empty(4096)]);
}

#[test]
fn a_lent_buffer_more_than_twice_the_room_is_not_used_and_goes_back_with_the_answer() {
    // Catches a job handed the buffer it was lent for a room it far exceeds, which the budget would charge in full
    // once put, and a lent buffer the job did not take dropped with it rather than given back to its actor.
    let mut engine = engine(|setup| setup);
    let turns: &[&[Order]] = &[&[Order::Put(8192)], &[Order::LetGo], &[Order::Draw(100)], &[Order::Take(8000)]];
    let builder = build(&mut engine, 1, turns);
    settle(&mut engine);

    assert_eq!(took(&engine, builder), [empty(100), empty(8192)]);
}

#[test]
fn a_job_that_fills_nothing_is_lent_nothing() {
    // Catches every job lent its actor's largest buffer, whatever its type, which takes the buffer from the actor for
    // as long as the job runs.
    let mut engine = engine(|setup| setup);
    let turns: &[&[Order]] = &[&[Order::Put(8192)], &[Order::LetGo], &[Order::Peek(8000)], &[Order::Take(8000)]];
    let builder = build(&mut engine, 1, turns);
    settle(&mut engine);

    assert_eq!(took(&engine, builder), [empty(8000), empty(8192)]);
}

#[test]
fn a_job_cancelled_before_it_is_handed_out_gives_back_what_it_was_lent() {
    // Catches the buffer lent a job dropped with the job when it is cancelled while it waits to be handed out.
    let mut engine = engine(|setup| setup);
    let turns: &[&[Order]] =
        &[&[Order::Put(8192)], &[Order::LetGo], &[Order::DrawCancelled(8000)], &[Order::Take(8000)]];
    let builder = build(&mut engine, 1, turns);
    settle(&mut engine);

    assert_eq!(took(&engine, builder), [empty(8192)]);
}
