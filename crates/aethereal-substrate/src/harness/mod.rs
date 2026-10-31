//! A harness: a host and its outside, which a test owns and steps, asking and being asked as any actor is (ADR-0036).

#[cfg(test)]
mod tests;

pub use crate::engine::outside::{Arrival, Handed, Ran, Turned};

use crate::engine::outside::{Reached, Unsent};
use crate::program::{Bootstraps, Builds, Program, ProgramError};
use crate::{Clock, Engine, table};
use aethereal_actor::{
    Actor, ActorRef, Actors, Answered, Carries, Declared, Handled, Handles, Ordered, Owed, Protocol, ProtocolRef,
    Proven, Reaches, Refused, Root, SeatNumber, Seated, Tie, Time, Unanswered,
};
use aethereal_data::{ActorPath, Decode, DecodeError, Encode, EncodeError, Kind, KindId, PathSegment};
use core::any;
use core::marker::PhantomData;
use core::time::Duration;
use crossbeam_queue::SegQueue;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

/// How long a harness's waiting verbs go on before they refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    /// A count of steps: for a run the test alone moves, the same every time.
    Steps(u64),
    /// Real time: for a host whose own threads do work the test waits on.
    Real(Duration),
}

/// A host a harness steps (ADR-0036).
pub trait Steps {
    /// How long a waiting verb goes on over this host.
    const BUDGET: Budget;

    /// The engine, between steps.
    fn engine(&mut self) -> &mut Engine;

    /// Brings in what is waiting, and runs one step.
    fn run_step(&mut self);
}

/// A clock that never moves, for an engine whose time the test moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// A bare engine, stepped on the test's thread at a time the test moves.
pub struct Bench {
    engine: Engine,
    now: Time,
}

impl Steps for Bench {
    const BUDGET: Budget = Budget::Steps(1 << 12);

    fn engine(&mut self) -> &mut Engine {
        &mut self.engine
    }

    fn run_step(&mut self) {
        self.engine.bring();
        self.engine.step(self.now, &Stopped);
    }
}

/// Why a harness's verb came to nothing. It names what it wanted, so a test that fails reads where it stood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheard {
    /// The program did not start.
    Program(String),
    /// The engine refused to seat the outside, or a root.
    Refused(Refused),
    /// What was sent does not encode.
    Unwritten(EncodeError),
    /// What arrived does not decode as what was asked for.
    Unread(DecodeError),
    /// The budget was spent with nothing of `wanted` arrived, after `steps` steps, and `waiting` pieces of mail left.
    Spent { wanted: &'static str, steps: u64, waiting: usize },
    /// Mail of `found` is what came where mail of `wanted` was spent on.
    OtherKind { wanted: KindId, found: KindId },
    /// The actor has no state here: it has ended, or its state lies in a guest's instance.
    NoState,
    /// The turn was handed no mail.
    NoMail,
    /// The asker waits on an answer of `wanted`, and was answered with `found`.
    OtherAnswer { wanted: KindId, found: KindId },
}

impl From<Refused> for Unheard {
    fn from(refused: Refused) -> Self {
        Self::Refused(refused)
    }
}

impl From<EncodeError> for Unheard {
    fn from(error: EncodeError) -> Self {
        Self::Unwritten(error)
    }
}

impl From<DecodeError> for Unheard {
    fn from(error: DecodeError) -> Self {
        Self::Unread(error)
    }
}

/// A request the outside asked, typed by what it is answered with.
#[must_use = "a request asked is read with `Harness::answer`"]
pub struct Asked<O> {
    count: u64,
    of: PhantomData<fn() -> O>,
}

/// A request asked of the outside, which waits on [`Harness::reply`] or [`Harness::unanswered`]: its contents are
/// [`Self::request`]. One the test never answers waits, since the outside never ends.
#[must_use = "a request held is answered, or its asker waits"]
pub struct Held<K> {
    /// What was asked.
    pub request: K,
    owed: Owed,
}

/// What a harness can ask: a reference whose target answers the kind `K` with one kind (ADR-0016).
pub trait Askable<K: Kind>: Proven {
    /// The one kind it is answered with.
    type Answer: Kind;
}

impl<T, K, A> Askable<K> for ActorRef<T>
where
    T: Actor + Handles<K> + Carries<K, Answers = (A,)>,
    K: Kind + Handled,
    A: Kind,
{
    type Answer = A;
}

impl<P, K, A> Askable<K> for ProtocolRef<P>
where
    P: Protocol + Carries<K, Answers = (A,)>,
    K: Kind,
    A: Kind,
{
    type Answer = A;
}

/// What a wait has spent of its budget.
struct Waited {
    began: Instant,
    steps: u64,
}

impl Waited {
    /// A wait beginning now.
    fn begun() -> Self {
        #[allow(clippy::disallowed_methods)] // A wait on a host's own threads runs on real time.
        let began = Instant::now();

        Self { began, steps: 0 }
    }
}

/// `Harness::answers` takes a shape of tickets and gives back the same shape of answers: an [`Asked`], a tuple of what is
/// [`Tickets`], or a `Vec` of `Asked` of one type.
pub trait Tickets {
    /// What each comes to, in the same shape.
    type Answers;

    /// Shows `visit` each request's count, and the name of what it is answered with.
    fn each(&self, visit: &mut dyn FnMut(u64, &'static str));

    /// Reads each, every one of which has arrived.
    ///
    /// # Errors
    ///
    /// Why what came does not read.
    fn read<H: Steps>(self, harness: &mut Harness<H>) -> Result<Self::Answers, Unheard>;
}

impl<O: Kind + Decode<Actors>> Tickets for Asked<O> {
    type Answers = Answered<O>;

    fn each(&self, visit: &mut dyn FnMut(u64, &'static str)) {
        visit(self.count, any::type_name::<O>());
    }

    fn read<H: Steps>(self, harness: &mut Harness<H>) -> Result<Answered<O>, Unheard> {
        harness.answer(self)
    }
}

impl<O: Kind + Decode<Actors>> Tickets for Vec<Asked<O>> {
    type Answers = Vec<Answered<O>>;

    fn each(&self, visit: &mut dyn FnMut(u64, &'static str)) {
        for asked in self {
            asked.each(visit);
        }
    }

    fn read<H: Steps>(self, harness: &mut Harness<H>) -> Result<Vec<Answered<O>>, Unheard> {
        self.into_iter().map(|asked| harness.answer(asked)).collect()
    }
}

macro_rules! tickets {
    ($($ticket:ident),+) => {
        impl<$($ticket: Tickets),+> Tickets for ($($ticket,)+) {
            type Answers = ($($ticket::Answers,)+);

            #[allow(non_snake_case, reason = "each binding is named for its type")]
            fn each(&self, visit: &mut dyn FnMut(u64, &'static str)) {
                let ($($ticket,)+) = self;
                $($ticket.each(visit);)+
            }

            #[allow(non_snake_case, reason = "each binding is named for its type")]
            fn read<H: Steps>(self, harness: &mut Harness<H>) -> Result<Self::Answers, Unheard> {
                let ($($ticket,)+) = self;

                Ok(($($ticket.read(harness)?,)+))
            }
        }
    };
}

tickets!(A, B);
tickets!(A, B, C);
tickets!(A, B, C, D);
tickets!(A, B, C, D, E);
tickets!(A, B, C, D, E, F);

/// A host and its outside (ADR-0036).
pub struct Harness<H> {
    host: H,
    outside: Seated,
    arrivals: Arc<SegQueue<Arrival>>,
    /// What arrived and no verb has taken, in the order it arrived.
    queue: VecDeque<Arrival>,
}

impl Harness<Bench> {
    /// A harness over the bare engine `engine`, at time nought.
    ///
    /// # Errors
    ///
    /// Why the engine refused to seat the outside.
    pub fn bare(engine: Engine) -> Result<Self, Unheard> {
        Self::over(Bench { engine, now: Time::ZERO })
    }

    /// Moves the test's clock on by `by`, and says a cycle is due at the time it reaches (ADR-0020).
    pub fn advance(&mut self, by: Duration) {
        let nanos = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        self.host.now = Time::from_nanos(self.host.now.nanos().saturating_add(nanos));
        let now = self.host.now;
        self.host.engine.cycle(now);
    }
}

impl Harness<Bench> {
    /// Steps until the engine has nothing to run, and answers with every turn that ran, but those of the outside, step
    /// by step. Only over a bare engine, which nothing moves between steps: a host's own threads may bring work to an
    /// idle engine, so there idle is no end.
    pub fn settle_turns(&mut self) -> Vec<Turned> {
        let _ = self.host.engine().take_turned();
        self.settle();

        self.turns()
    }
}

impl<H: Steps + Builds> Harness<H> {
    /// A harness over the program `program`, built and started.
    ///
    /// # Errors
    ///
    /// Why the program did not start, or the engine refused to seat the outside.
    pub fn of<R, I: 'static, O>(program: Program<H, R>) -> Result<Self, Unheard>
    where
        R: Bootstraps<H, I> + Ordered<O> + 'static,
    {
        program.build().map_err(|error: ProgramError| Unheard::Program(error.to_string())).and_then(Self::over)
    }
}

impl<H: Steps> Harness<H> {
    /// `host`, its engine started, with the outside seated.
    ///
    /// # Errors
    ///
    /// Why the engine refused to seat the outside.
    pub fn over(mut host: H) -> Result<Self, Unheard> {
        let (outside, arrivals) = host.engine().seat_outside()?;
        host.engine().start();

        Ok(Self { host, outside, arrivals, queue: VecDeque::new() })
    }

    /// The host, to reach what only it holds.
    pub const fn host(&mut self) -> &mut H {
        &mut self.host
    }

    /// The root of type `T`, which is a singleton.
    ///
    /// # Panics
    ///
    /// Panics when the program has no such root: a test that names one the program it built does not hold.
    pub fn root<T: Actor<Key = ()> + Root>(&mut self) -> ActorRef<T> {
        let path = ActorPath::from_root(&PathSegment::new(T::NAME, &()));
        let seated = table::reach(self.host.engine().table(), &path, Declared::of::<T>().id());
        let is_seated = seated.seat != SeatNumber::NOWHERE;
        assert!(is_seated, "the program has no root {}", T::NAME);

        ActorRef::found(seated)
    }

    /// Makes the root `T` from `config`, between steps: for a test's own actor, whose config may hold a
    /// [`Self::protocol`].
    ///
    /// # Errors
    ///
    /// Why the engine refused it.
    pub fn add_root<T: Root>(&mut self, key: &T::Key, config: T::Config) -> Result<(), Unheard> {
        self.host.engine().add_root::<T>(key, config)?;

        Ok(())
    }

    /// A reference to the outside as the protocol `P`, true because the outside takes every kind.
    #[must_use]
    pub const fn protocol<P>(&self) -> ProtocolRef<P> {
        ProtocolRef::found(self.outside)
    }

    /// Asks `kind` of `target`, and answers with what [`Self::answer`] reads its answer through.
    ///
    /// # Errors
    ///
    /// Why it does not encode.
    pub fn ask<R, K>(&mut self, target: R, kind: &K) -> Result<Asked<R::Answer>, Unheard>
    where
        R: Askable<K>,
        K: Kind + Encode<Actors>,
    {
        let answer = KindId::of::<R::Answer>();
        let count = self.host.engine().ask_outside(self.outside, target.seated(), kind, answer)?;

        Ok(Asked { count, of: PhantomData })
    }

    /// Sends `kind` to `target`, asking for nothing.
    ///
    /// # Errors
    ///
    /// Why it does not encode.
    pub fn tell<R, K>(&mut self, target: R, kind: &K) -> Result<(), Unheard>
    where
        R: Proven + Reaches<K>,
        K: Kind + Encode<Actors>,
    {
        self.host.engine().send_outside(self.outside, target.seated(), kind, |_| Tie::TELL)?;

        Ok(())
    }

    /// Steps until what became of `asked` has arrived, and reads it as its asker would.
    ///
    /// # Errors
    ///
    /// [`Unheard::Spent`] when nothing came, and why what came does not read as `O`.
    #[allow(clippy::needless_pass_by_value, reason = "an answer is read once, so it is taken")]
    pub fn answer<O: Kind + Decode<Actors>>(&mut self, asked: Asked<O>) -> Result<Answered<O>, Unheard> {
        let found = self.wait(&mut Waited::begun(), any::type_name::<O>(), settles(asked.count))?;

        self.read_answer(found)
    }

    /// Steps until what became of each of `asked` has arrived, and reads each as [`Self::answer`] does, in the shape
    /// they were given in: a tuple of tickets answers a tuple of answers, each of its own type, and a `Vec` of tickets of
    /// one type a `Vec` of answers. The budget covers the whole wait.
    ///
    /// # Errors
    ///
    /// [`Unheard::Spent`] naming the first that has not come, and why what came does not read.
    #[allow(clippy::needless_pass_by_value, reason = "an answer is read once, so its ticket is taken")]
    pub fn answers<T: Tickets>(&mut self, asked: T) -> Result<T::Answers, Unheard> {
        let mut spent = Waited::begun();
        let mut wanted = Vec::new();
        asked.each(&mut |count, name| wanted.push((count, name)));
        for (count, name) in wanted {
            self.wait(&mut spent, name, settles(count))?;
        }

        asked.read(self)
    }

    /// The answer at `at` in the queue, taken and read as `O`.
    fn read_answer<O: Kind + Decode<Actors>>(&mut self, at: usize) -> Result<Answered<O>, Unheard> {
        let Some(arrival) = self.queue.remove(at) else {
            unreachable!("what was found is still there");
        };
        if let Reached::Unanswered(_) = arrival.reached() {
            return read_as::<Unanswered>(&arrival).map(Answered::No);
        }

        read_as(&arrival).map(Answered::Yes)
    }

    /// Steps until mail of kind `K` asked of the outside has arrived, and answers with it, held.
    ///
    /// # Errors
    ///
    /// [`Unheard::OtherKind`] when another request came and none of `K`, and [`Unheard::Spent`] when none did.
    pub fn asked<K: Kind + Decode<Actors>>(&mut self) -> Result<Held<K>, Unheard> {
        let is_request = |arrival: &Arrival| matches!(arrival.reached(), Reached::Asked(_));
        let taken = self.take::<K>(is_request)?;
        let Reached::Asked(owed) = taken.reached() else {
            unreachable!("only a request is taken");
        };
        let request = read_as(&taken)?;

        Ok(Held { request, owed })
    }

    /// Steps until mail of kind `K` told to the outside has arrived, and reads it.
    ///
    /// # Errors
    ///
    /// [`Unheard::OtherKind`] when other mail came and none of `K`, and [`Unheard::Spent`] when none did.
    pub fn heard<K: Kind + Decode<Actors>>(&mut self) -> Result<K, Unheard> {
        let is_told = |arrival: &Arrival| arrival.reached() == Reached::Told;

        read_as(&self.take::<K>(is_told)?)
    }

    /// Answers `held` with `answer`.
    ///
    /// # Errors
    ///
    /// [`Unheard::OtherAnswer`] when the asker waits on another kind, and why it does not encode.
    #[allow(clippy::needless_pass_by_value, reason = "a request is answered once, so it is taken")]
    pub fn reply<K, A: Kind + Encode<Actors>>(&mut self, held: Held<K>, answer: &A) -> Result<(), Unheard> {
        self.host.engine().answer_outside(held.owed, answer).map_err(|unsent| match unsent {
            Unsent::Unwritten(error) => Unheard::Unwritten(error),
            Unsent::OtherAnswer { wanted, found } => Unheard::OtherAnswer { wanted, found },
        })
    }

    /// Lets `held` go, its asker told `why`: as a receiver that ended, failed, or stayed silent would leave it.
    ///
    /// # Errors
    ///
    /// Never, in practice: a reason is one byte.
    #[allow(clippy::needless_pass_by_value, reason = "a request is let go of once, so it is taken")]
    pub fn unanswered<K>(&mut self, held: Held<K>, why: Unanswered) -> Result<(), Unheard> {
        self.host.engine().unanswered_outside(held.owed, why)?;

        Ok(())
    }

    /// The ids of the kinds of what has arrived and no verb has taken, in the order it arrived, emptying it: so a test
    /// can assert that nothing else came.
    pub fn rest(&mut self) -> Vec<KindId> {
        self.drain();

        self.queue.drain(..).map(|arrival| arrival.kind()).collect()
    }

    /// What has arrived and no verb has taken, whole, in the order it arrived, emptying it: for a test that reads the
    /// order of mail of several kinds, as answers among events, which a verb that takes one kind would lose.
    pub fn arrived(&mut self) -> Vec<Arrival> {
        self.drain();

        self.queue.drain(..).collect()
    }

    /// Runs one step.
    pub fn step(&mut self) {
        self.host.run_step();
        self.drain();
    }

    /// Runs one step, and answers with the turns it ran, but those of the outside.
    pub fn step_turns(&mut self) -> Vec<Turned> {
        let _ = self.host.engine().take_turned();
        self.step();

        self.turns()
    }

    /// The turns recorded since they were last taken, but those of the outside.
    fn turns(&mut self) -> Vec<Turned> {
        let outside = self.outside;
        let mut turned = self.host.engine().take_turned();
        turned.retain(|turn| turn.actor != outside);

        turned
    }

    /// The state of the actor `actor` refers to, as of the last step.
    ///
    /// # Errors
    ///
    /// [`Unheard::NoState`] when it has ended, or its state lies in a guest's instance.
    pub fn state<T: 'static>(&mut self, actor: ActorRef<T>) -> Result<&T, Unheard> {
        self.host.engine().state(actor.seated()).ok_or(Unheard::NoState)
    }

    /// Steps until the engine has nothing to run.
    pub fn settle(&mut self) {
        self.step();
        while !self.host.engine().is_idle() {
            self.step();
        }
    }

    /// The first mail of `K` that `is_it`, once it has arrived.
    fn take<K: Kind>(&mut self, is_it: impl Fn(&Arrival) -> bool) -> Result<Arrival, Unheard> {
        let kind = KindId::of::<K>();
        let is_kind = |arrival: &Arrival| is_it(arrival) && arrival.kind() == kind;
        let found = self.wait(&mut Waited::begun(), any::type_name::<K>(), is_kind);
        let at = match found {
            Ok(at) => at,
            Err(spent) => {
                let other = self.queue.iter().find(|arrival| is_it(arrival));
                let refusal = other.map_or(spent, |arrival| Unheard::OtherKind { wanted: kind, found: arrival.kind() });

                return Err(refusal);
            }
        };
        let Some(arrival) = self.queue.remove(at) else {
            unreachable!("what was found is still there");
        };

        Ok(arrival)
    }

    /// Steps until an arrival `is_it`, and answers with where it lies in the queue, counting what it spends in `spent`.
    fn wait(
        &mut self,
        spent: &mut Waited,
        wanted: &'static str,
        is_it: impl Fn(&Arrival) -> bool,
    ) -> Result<usize, Unheard> {
        loop {
            self.drain();
            if let Some(at) = self.queue.iter().position(&is_it) {
                return Ok(at);
            }
            let is_spent = match H::BUDGET {
                Budget::Steps(most) => spent.steps >= most,
                Budget::Real(most) => spent.began.elapsed() >= most,
            };
            if is_spent {
                return Err(Unheard::Spent { wanted, steps: spent.steps, waiting: self.queue.len() });
            }
            self.host.run_step();
            spent.steps += 1;
            if matches!(H::BUDGET, Budget::Real(_)) {
                thread::yield_now();
            }
        }
    }

    /// Moves what has arrived into the queue.
    fn drain(&mut self) {
        while let Some(arrival) = self.arrivals.pop() {
            self.queue.push_back(arrival);
        }
    }
}

impl Turned {
    /// The mail it was handed, read as `K`.
    ///
    /// # Errors
    ///
    /// [`Unheard::NoMail`] when it was handed none, [`Unheard::OtherKind`] when the mail is of another kind, and why it
    /// does not decode.
    pub fn read<K: Kind + Decode<Actors>>(&self) -> Result<K, Unheard> {
        let Handed::Mail(arrival) = &self.handed else {
            return Err(Unheard::NoMail);
        };

        read_as(arrival)
    }
}

/// Whether an arrival says what became of the request of the count `count`.
const fn settles(count: u64) -> impl Fn(&Arrival) -> bool {
    move |arrival| matches!(arrival.reached(), Reached::Answered(of) | Reached::Unanswered(of) if of == count)
}

/// `arrival`, known to be of kind `K`, read.
fn read_as<K: Kind + Decode<Actors>>(arrival: &Arrival) -> Result<K, Unheard> {
    arrival.read::<K>()?.ok_or_else(|| Unheard::OtherKind { wanted: KindId::of::<K>(), found: arrival.kind() })
}
