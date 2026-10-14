//! The engine: it runs turns in steps, and applies what they asked for (ADR-0009, ADR-0011).

mod begun;
mod costs;
mod plan;
mod since;
#[cfg(test)]
mod tests;

pub use costs::Tally;
pub use plan::{Plan, Share, Workers};

use crate::id::root_id;
use crate::stream::{Pending, Streams, take};
use crate::watch::Watchers;
use crate::{ActorTable, Clock, Context, Entry, Envelope, Folded, Log};
use aethereal_actor::{
    Actor, Arena, Asked, Beginning, Code, Effect, Ending, Fault, How, Item, Mail, MailId, Refused, Runs, Stood, Turn,
    What, Why,
};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use begun::begun;
use costs::{CostKey, Costs, ItemKind, KindIndex, Kinds, toward};
use plan::{Job, Work};
use since::Since;
use std::collections::BTreeMap;
use std::iter;
use std::mem;
use std::num::NonZeroUsize;

/// The engine: its actors, the views of them, and the steps their turns run in (ADR-0011).
///
/// A step runs every actor that has something to react to, one turn for each
/// item in its stream as the step began. When the turns have finished the
/// engine folds their effects into the views, each with the context it was
/// emitted in. What a step's turns sent is in the streams for a later step.
///
/// A step is planned, run, and applied. [`Engine::plan`] takes each actor
/// with something to run out of the engine and shares the actors among the
/// workers; each worker runs its share through [`Engine::reading`]; and
/// [`Engine::apply`] puts the actors back and folds what their turns asked
/// for, in the order of their ids, whichever worker ran them. [`Engine::step`]
/// does all three on the thread that calls it: the whole of a browser host's
/// engine (ADR-0001).
pub struct Engine {
    /// Which actors there are: the first view.
    table: ActorTable,
    /// Who watches each actor.
    watchers: Folded<Watchers>,
    /// What each actor has to react to.
    streams: Folded<Streams>,
    /// What each actor logged most recently.
    log: Folded<Log>,
    /// Every actor the engine still runs, under its id: the live ones, and the ended ones whose closing turn is
    /// still to run.
    seats: BTreeMap<u64, Seat>,
    /// The actors themselves, each under its id. An actor leaves when its closing turn takes it.
    actors: BTreeMap<u64, Box<dyn Runs>>,
    /// Where mail is written: for each parity of step, an arena for each writer, the first for mail sent between
    /// steps.
    arenas: [Vec<Arena>; 2],
    /// The number given to each kind of actor seated so far.
    kinds: Kinds,
    /// How long turns take, as measured so far.
    costs: Costs,
    /// The expected work of recent steps, in nanoseconds: a moving average weighted to the most recent.
    recent: u64,
    /// The number of the last step run. No step has run at zero.
    step: u64,
    /// The most bytes one read of a piece of mail may allocate.
    read_limit: usize,
}

/// What the engine keeps for one actor beside the actor itself.
struct Seat {
    /// Where the actor stands.
    path: ActorPath,
    /// Its kind, as the engine numbers it.
    kind: KindIndex,
    /// How many pieces of mail it has sent, as of the last step applied.
    sent: u64,
    /// Whether it still reacts.
    life: Life,
}

/// Whether an actor the engine runs still reacts.
enum Life {
    /// It reacts to its stream.
    Live,
    /// It has ended, `how` it ended, and its closing turn runs in the next step, in `context`.
    Ending { how: How, context: Context },
}

/// The engine as a step's workers read it while they run: the table, and each actor's seat (ADR-0011).
pub struct Reading<'a> {
    table: &'a ActorTable,
    seats: &'a BTreeMap<u64, Seat>,
    /// The arenas the last step's mail was written to, by writer.
    mail: &'a [Arena],
    read_limit: usize,
}

/// What one worker's share of a step left: each actor's turns, and how long they took.
pub struct Done {
    actors: Vec<Turned>,
    tally: Tally,
    /// The arena the worker wrote its mail to, to keep for the step after next.
    arena: Arena,
}

/// One actor's turns of a step, and the actor when it is still the engine's to run.
struct Turned {
    id: ActorId,
    ran: Vec<Ran>,
    kept: Kept,
}

/// Whether an actor goes back into the engine after its turns of a step.
enum Kept {
    /// It does.
    Live(Box<dyn Runs>),
    /// Its closing turn took it.
    Gone,
}

/// One finished turn, as the engine applies it.
struct Ran {
    /// What the turn asked for, in order.
    effects: Vec<Effect>,
    /// The children it stood up.
    stood: Vec<Stood>,
    /// The context its effects were emitted in.
    context: Context,
}

impl Engine {
    /// An engine with no actors, its views in `parts` parts, whose turns read at most `read_limit` bytes into
    /// any one piece of mail.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize) -> Self {
        Self {
            table: ActorTable::new(parts),
            watchers: Folded::new(parts),
            streams: Folded::new(parts),
            log: Folded::new(parts),
            seats: BTreeMap::new(),
            actors: BTreeMap::new(),
            arenas: [Vec::new(), Vec::new()],
            kinds: Kinds::new(),
            costs: Costs::new(),
            recent: 0,
            step: 0,
            read_limit,
        }
    }

    /// The table of actors, as of the last step.
    #[must_use]
    pub const fn table(&self) -> &ActorTable {
        &self.table
    }

    /// What the actor `id` stands for logged most recently, oldest first, as of the last step.
    ///
    /// It holds the fault of a turn that failed, and of a closing turn that did. Nothing for an actor that has
    /// logged nothing.
    pub fn log(&self, id: ActorId) -> impl Iterator<Item = &Entry> {
        self.log.shard(id.bits()).get(&id.bits()).into_iter().flatten()
    }

    /// The number of the last step run, from one. Zero before the first.
    #[must_use]
    pub const fn step_number(&self) -> u64 {
        self.step
    }

    /// Whether a step would run no turn: no actor has anything to react to, and none has a closing turn to run.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.seats.iter().all(|(bits, seat)| {
            let has_items = self.has_items(*bits);
            let is_ending = matches!(seat.life, Life::Ending { .. });

            !(has_items || is_ending)
        })
    }

    /// Makes a root of type `A` under `key`, for the chassis, and answers with its id (ADR-0008).
    ///
    /// The root's `begin` runs here, and what it asked for is applied before this returns, as a step's effects
    /// are. A root is made between steps.
    ///
    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds for another path (ADR-0008).
    pub fn add_root<A: Actor + 'static>(&mut self, key: &A::Key, config: A::Config) -> Result<ActorId, Refused> {
        let segment = PathSegment::new(A::NAME, key);
        let id = root_id(&segment);
        let path = ActorPath::from_root(&segment);
        let is_taken = self.table.path(id).as_ref() == Some(&path);
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        // Mail sent between steps is read in the next step, with the mail the last step's turns sent.
        let parity = parity(self.step);
        let mut asked = Asked::writing(self.arena(parity, ROOTS));
        let begun = A::begin(config, &mut Beginning::new(id, path.clone(), &self.table, &mut asked));
        let (asked, stood, arena) = asked.into_parts();
        self.arenas[parity][ROOTS] = arena;
        let root = begun.map_err(Refused::Began)?;
        let from = Code::Linked(A::NAME);
        let made = match segment.key() {
            None => What::Root { from },
            Some(key) => What::RootInstanced { from, key: key.clone() },
        };
        let effects = iter::once(Effect::new(id, made)).chain(asked).collect();
        let context = Context { cause: None, root: MailId { sender: id, count: 0 }, depth: 1 };

        self.seat(id, path, Box::new(root));
        self.fold(vec![Ran { effects, stood, context }], &[]);

        Ok(id)
    }

    /// Runs one step on the thread that calls it: plans it for one worker, runs that worker's share, and applies
    /// what it left (ADR-0011).
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn step(&mut self, clock: &impl Clock) {
        let shares = self.plan(Workers::ONE).into_shares();
        let reading = self.reading();
        let done = shares.into_iter().map(|share| reading.run(share, clock)).collect();

        self.apply(done);
    }

    /// Begins a step on `workers`: takes each actor with something to run out of the engine, with what it runs,
    /// and shares the actors among the workers that are to run, by what their turns are expected to cost
    /// (ADR-0011).
    ///
    /// An actor that has ended runs its closing turn, and what was in its stream is dropped.
    pub fn plan(&mut self, workers: Workers) -> Plan {
        self.step += 1;

        let mut jobs = Vec::new();
        let mut costs = Vec::new();
        let ids: Vec<ActorId> = self.seats.keys().map(|bits| ActorId::new(*bits)).collect();
        for id in ids {
            let items: Vec<Pending> = self.take_items(id).collect();
            let Some(seat) = self.seats.get(&id.bits()) else {
                continue;
            };
            let work = match seat.life {
                Life::Ending { .. } => Work::Closing,
                Life::Live if items.is_empty() => continue,
                Life::Live => Work::Items(items),
            };
            let Some(actor) = self.actors.remove(&id.bits()) else {
                continue;
            };
            let kind = seat.kind;

            costs.push(self.expect(kind, &work));
            jobs.push(Job { id, kind, actor, work });
        }

        let work = costs.iter().sum();
        let mut planned = plan::plan(jobs, &costs, self.recent, workers);
        self.recent = toward(self.recent, work);

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer.
        let parity = parity(self.step);
        for arena in &mut self.arenas[parity] {
            arena.clear();
        }
        for (share, slot) in planned.shares_mut().zip(ROOTS + 1..) {
            let arena = self.arena(parity, slot);
            share.write_to(arena);
        }

        planned
    }

    /// The engine as the step's workers read it while they run their shares.
    #[must_use]
    pub fn reading(&self) -> Reading<'_> {
        let mail = &self.arenas[parity(self.step + 1)];

        Reading { table: &self.table, seats: &self.seats, mail, read_limit: self.read_limit }
    }

    /// Ends a step: puts back the actors the workers ran, takes in how long their turns took, and applies what
    /// the turns asked for, in the order of the actors' ids (ADR-0011).
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply(&mut self, done: Vec<Done>) {
        let mut turned = Vec::new();
        let parity = parity(self.step);
        for Done { actors, tally, arena } in done {
            self.costs.merge(tally);
            turned.extend(actors);
            let slot = arena.slot();
            self.arenas[parity][slot] = arena;
        }
        turned.sort_by_key(|turned| turned.id.bits());

        let mut ran = Vec::new();
        let mut gone = Vec::new();
        for Turned { id, ran: turns, kept } in turned {
            match kept {
                Kept::Live(actor) => {
                    self.actors.insert(id.bits(), actor);
                }
                Kept::Gone => gone.push(id),
            }
            ran.extend(turns);
        }

        self.fold(ran, &gone);
    }

    /// What the turns of the job of an actor of kind `kind` that runs `work` are expected to cost, in nanoseconds.
    fn expect(&self, kind: KindIndex, work: &Work) -> u64 {
        let key = |item| CostKey { actor: kind, item };
        match work {
            Work::Closing => self.costs.expect(&key(ItemKind::Closing)),
            Work::Items(items) => items.iter().map(|pending| self.costs.expect(&key(item_kind(pending)))).sum(),
        }
    }

    /// Folds finished turns, in the order given (ADR-0011).
    ///
    /// Every turn's effects are folded into the table and the watchers, and its children made live, before any is
    /// folded into the streams, which read both: so a child spawned in a step is there for mail sent to it in that
    /// step, and mail to an actor that ended in the step is not kept. The actors in `gone` have run their closing
    /// turn, and leave last.
    fn fold(&mut self, ran: Vec<Ran>, gone: &[ActorId]) {
        let ran: Vec<Ran> = ran.into_iter().map(|ran| Ran { effects: begun(ran.effects), ..ran }).collect();

        let mut folded = Vec::with_capacity(ran.len());
        for Ran { effects, stood, context } in ran {
            self.table.fold(&effects, &context);
            self.watchers.fold(&effects, &context, ());
            self.log.fold(&effects, &context, ());

            for child in stood {
                self.seat(child.id(), child.path().clone(), child.into_actor());
            }
            for effect in &effects {
                self.count(effect);
                if let Some(how) = ending(effect, effect.by()) {
                    self.end(effect.by(), how, context);
                }
            }

            folded.push((effects, context));
        }
        for (effects, context) in folded {
            self.streams.fold_owned(effects, &context, (&self.table, &self.watchers));
        }

        for id in gone {
            self.seats.remove(&id.bits());
            // Whatever reached it after its closing turn is dropped with it.
            self.streams.shard_mut(id.bits()).remove(&id.bits());
        }
    }

    /// The arena of the writer numbered `slot` for steps of `parity`, taken out to write to; made when it is the
    /// first.
    fn arena(&mut self, parity: usize, slot: usize) -> Arena {
        let arenas = &mut self.arenas[parity];
        while arenas.len() <= slot {
            arenas.push(Arena::new(arenas.len()));
        }

        mem::replace(&mut arenas[slot], Arena::new(slot))
    }

    /// Gives the actor `id` stands for, at `path`, a seat: it is run from the next step.
    fn seat(&mut self, id: ActorId, path: ActorPath, actor: Box<dyn Runs>) {
        let kind = self.kinds.number(path.last().kind());
        self.seats.insert(id.bits(), Seat { path, kind, sent: 0, life: Life::Live });
        self.actors.insert(id.bits(), actor);
    }

    /// Marks the actor `id` stands for as ended, `how` it ended: its closing turn runs in the next step, in the
    /// chain of the mail that brought its ending about (ADR-0012).
    fn end(&mut self, id: ActorId, how: How, context: Context) {
        if let Some(seat) = self.seats.get_mut(&id.bits()) {
            seat.life = Life::Ending { how, context };
        }
    }

    /// Keeps the count of a send applied: the next turn of its sender counts on from it.
    fn count(&mut self, effect: &Effect) {
        let What::Send { id, .. } = effect.what() else {
            return;
        };
        if let Some(seat) = self.seats.get_mut(&id.sender.bits()) {
            seat.sent = seat.sent.max(id.count);
        }
    }

    /// Whether the actor whose bits these are has anything in its stream.
    fn has_items(&self, bits: u64) -> bool {
        self.streams.shard(bits).get(&bits).is_some_and(|items| !items.is_empty())
    }

    /// Takes everything in the stream of the actor `id` stands for, in the order it is to be handed it.
    fn take_items(&mut self, id: ActorId) -> impl Iterator<Item = Pending> + use<> {
        let bits = id.bits();
        let taken = self.streams.shard_mut(bits).remove(&bits);

        taken.into_iter().flat_map(take)
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: each actor's closing turn when it has ended, and otherwise one turn for
    /// each item it was given, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was given behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let mut tally = Tally::default();
        let (jobs, mut arena) = share.into_parts();
        let actors = jobs.into_iter().map(|job| self.turns(job, clock, &mut tally, &mut arena)).collect();

        Done { actors, tally, arena }
    }

    /// The turns of one actor's job, each measured into `tally`, their mail written to `arena`.
    fn turns(&self, job: Job, clock: &impl Clock, tally: &mut Tally, arena: &mut Arena) -> Turned {
        let Job { id, kind, actor, work } = job;
        let Some(seat) = self.seats.get(&id.bits()) else {
            return Turned { id, ran: Vec::new(), kept: Kept::Live(actor) };
        };
        let key = |item| CostKey { actor: kind, item };

        match (work, &seat.life) {
            (Work::Closing, Life::Ending { how, context }) => {
                let mut asked = Asked::writing(mem::take(arena));
                let why = why(how);
                let start = clock.nanos();
                let ended = actor.end(&mut Ending::new(id, seat.path.clone(), self.table, &mut asked, seat.sent, why));
                tally.add(key(ItemKind::Closing), clock.nanos().saturating_sub(start));
                // A closing turn that fails asks for nothing: only its log lines are kept, and the engine adds its
                // fault to them (ADR-0009). Either way the engine adds that the actor's life is over, after what the
                // turn sent.
                let (mut effects, _, written) = asked.into_parts();
                *arena = written;
                if let Err(fault) = ended {
                    effects.retain(is_line);
                    effects.push(Effect::new(id, What::EndFailed(fault)));
                }
                effects.push(Effect::new(id, What::Ended(how.clone())));

                Turned { id, ran: vec![Ran { effects, stood: Vec::new(), context: *context }], kept: Kept::Gone }
            }
            (Work::Items(items), Life::Live) => {
                let mut actor = actor;
                let mut ran = Vec::new();
                let mut stood_since = Vec::new();
                let mut count = seat.sent;
                for pending in items {
                    let table = Since { table: self.table, stood: &stood_since };
                    let mut asked = Asked::writing(mem::take(arena));
                    let source = pending.source();
                    let (item, context) = match &pending {
                        Pending::Mail(envelope) => {
                            let Envelope { id: cause, kind, bytes, root, depth, .. } = envelope;
                            let bytes =
                                self.mail.get(bytes.slot).and_then(|arena| arena.read(*bytes)).unwrap_or_default();
                            let mail = Mail::new(*kind, bytes, source, *root, *depth, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(*cause), root: *root, depth: depth + 1 })
                        }
                        Pending::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(id, seat.path.clone(), &table, &mut asked, count, source);
                    let start = clock.nanos();
                    let handled = actor.handle(item, &mut turn);
                    tally.add(key(item_kind(&pending)), clock.nanos().saturating_sub(start));
                    let (turned, written) = Ran::of(id, handled, asked, context);
                    *arena = written;

                    count = turned.sent(id).unwrap_or(count);
                    stood_since.extend(turned.stood.iter().map(|child| (child.id(), child.path().clone())));
                    let over = turned.ends(id);
                    ran.push(turned);
                    if over {
                        break;
                    }
                }

                Turned { id, ran, kept: Kept::Live(actor) }
            }
            // A plan gives a closing turn only to an actor that has ended, and items only to a live one.
            _ => Turned { id, ran: Vec::new(), kept: Kept::Live(actor) },
        }
    }
}

impl Ran {
    /// The turn of the actor `id` stands for that returned `outcome`, having asked for `asked`, in `context`.
    ///
    /// A turn that fails changes nothing outside its actor: what it asked for is dropped but for its log lines,
    /// and the engine writes the one effect that says the actor ended (ADR-0009).
    /// The arena the turn's mail was written to comes back beside it.
    fn of(id: ActorId, outcome: Result<(), Fault>, asked: Asked, context: Context) -> (Self, Arena) {
        let (mut effects, mut stood, arena) = asked.into_parts();
        if let Err(fault) = outcome {
            effects.retain(is_line);
            effects.push(Effect::new(id, What::Failed(fault)));
            stood.clear();
        }

        (Self { effects, stood, context }, arena)
    }

    /// Whether the turn ends the actor `id` stands for.
    fn ends(&self, id: ActorId) -> bool {
        self.effects.iter().any(|effect| ending(effect, id).is_some())
    }

    /// The count of the last mail the actor `id` stands for sent in this turn, or none when it sent none.
    fn sent(&self, id: ActorId) -> Option<u64> {
        self.effects
            .iter()
            .filter_map(|effect| match effect.what() {
                What::Send { id: mail, .. } if mail.sender == id => Some(mail.count),
                _ => None,
            })
            .max()
    }
}

/// What a pending item is, as far as the cost of reacting to it goes.
const fn item_kind(pending: &Pending) -> ItemKind {
    match pending {
        Pending::Mail(envelope) => ItemKind::Mail(envelope.kind),
        Pending::Ended { .. } => ItemKind::Ended,
    }
}

/// The writer of the mail sent between steps, by the chassis: the first.
const ROOTS: usize = 0;

/// Which of the two arenas of each writer step `step` writes.
const fn parity(step: u64) -> usize {
    (step % 2) as usize
}

/// Whether `effect` is a logged line: what is kept of a turn that failed (ADR-0009).
fn is_line(effect: &Effect) -> bool {
    matches!(effect.what(), What::Log { .. })
}

/// How the actor `id` stands for ends, when `effect` is that actor's own ending.
fn ending(effect: &Effect, id: ActorId) -> Option<How> {
    let is_own = effect.by() == id;
    match effect.what() {
        What::Close if is_own => Some(How::Closed),
        What::Failed(fault) if is_own => Some(How::Failed(fault.clone())),
        _ => None,
    }
}

/// Why a closing turn runs, for an actor that ended `how`.
const fn why(how: &How) -> Why {
    match how {
        How::Failed(_) => Why::Faulted,
        How::Closed | How::Earlier => Why::Closed,
    }
}
