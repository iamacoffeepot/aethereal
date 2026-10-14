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
use crate::stream::{self, Pending, Streams};
use crate::table::Actors;
use crate::{ActorTable, ByActor, Clock, Context, Entry, Envelope, Folded, Lives, Log, Routed, State, lives};
use aethereal_actor::{
    Actor, Arena, Asked, Beginning, Code, Effect, Ending, Fault, How, Item, Mail, MailId, Payload, Refused, Runs,
    SeatNumber, Seated, Seats, Stood, Turn, What, Why,
};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use begun::begun;
use costs::{CostKey, Costs, ItemKind, KindIndex, Kinds, toward};
use plan::{Job, Work};
use since::Since;
use std::borrow::Cow;
use std::iter;
use std::mem;
use std::num::NonZeroUsize;
use std::ops::Range;

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
/// for. Every view folds the same in whatever order turns come, so it takes
/// each worker's turns as they are. [`Engine::step`]
/// does all three on the thread that calls it: the whole of a browser host's
/// engine (ADR-0001).
pub struct Engine {
    /// Which actors there are: the first view.
    table: ActorTable,
    /// What each actor has to react to.
    streams: Folded<Streams>,
    /// What each actor logged most recently.
    log: Folded<Log>,
    /// How far each actor's life has gone: the mail it has sent, and whether it has ended.
    lives: Folded<Lives>,
    /// A seat for every actor the engine still runs, the live ones and the ended ones whose closing turn is still to
    /// run, and the seats left empty, numbered from zero. An actor leaves its seat when its closing turn takes it.
    seats: Vec<Seat>,
    /// The actors themselves, by the number of their seat.
    actors: Vec<Held>,
    /// The empty seats, given out before new numbers are.
    empty: Vec<SeatNumber>,
    /// Where mail is written: for each parity of step, an arena for each writer, the first for mail sent between
    /// steps.
    arenas: [Vec<Arena>; 2],
    /// What each worker fills in a step, by writer, kept from one step to the next.
    lists: Vec<Lists>,
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

/// One seat.
enum Seat {
    /// No actor sits in it.
    Empty,
    /// This actor does.
    Taken(Sitting),
}

/// An actor in its seat: what is fixed when it is seated.
struct Sitting {
    id: ActorId,
    /// Where the actor stands.
    path: ActorPath,
    /// Its kind, as the engine numbers it.
    kind: KindIndex,
}

/// Whether the engine holds the actor of a seat.
enum Held {
    /// It does.
    Here(Box<dyn Runs>),
    /// It does not: the actor is out with a worker for the step, or the seat is empty.
    Away,
}

/// The engine as a step's workers read it while they run: the table, each actor's seat and life, and the items
/// handed out (ADR-0011).
pub struct Reading<'a> {
    table: &'a ActorTable,
    streams: &'a Folded<Streams>,
    lives: &'a Folded<Lives>,
    seats: &'a [Seat],
    /// The arenas the last step's mail was written to, by writer.
    mail: &'a [Arena],
    read_limit: usize,
}

/// What one worker's share of a step left: its turns, the actors it ran, and how long they took.
pub struct Done {
    lists: Lists,
    tally: Tally,
    /// The arena the worker wrote its mail to, to keep for the step after next.
    arena: Arena,
}

/// What a worker fills in a step: its row of buckets for each view, the children its turns stood up, and each actor
/// it ran, and the seats it gives the children. The engine empties it and hands it back the next step, so nothing in
/// it is allocated again.
#[derive(Default)]
pub struct Lists {
    /// The effects of the turn running, routed into the buckets when it finishes.
    effects: Vec<Effect>,
    /// Boxed, as the lists pass between threads each step.
    routes: Box<Routes>,
    stood: Vec<Stood>,
    actors: Vec<Turned>,
    seats: Seats,
}

/// One writer's row of buckets for each view.
#[derive(Default)]
struct Routes {
    table: Routed<Actors>,
    log: Routed<Log>,
    lives: Routed<Lives>,
    streams: Routed<Streams>,
}

/// An actor a worker ran, and the actor when it is still the engine's to run.
struct Turned {
    seat: SeatNumber,
    kept: Kept,
}

/// Whether an actor goes back into the engine after its turns of a step.
enum Kept {
    /// It does.
    Live(Box<dyn Runs>),
    /// Its closing turn took it.
    Gone,
}

/// One finished turn, as its worker routes it.
struct Ran {
    /// What the turn asked for, in order: a range of its worker's effects.
    effects: Range<usize>,
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
            streams: Folded::new(parts),
            log: Folded::new(parts),
            lives: Folded::new(parts),
            seats: Vec::new(),
            actors: Vec::new(),
            empty: Vec::new(),
            arenas: [Vec::new(), Vec::new()],
            lists: Vec::new(),
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
    /// logged nothing, or whose seat another actor has taken and logged in since.
    pub fn log(&self, id: ActorId) -> impl Iterator<Item = &Entry> {
        let actor = self.table.seat(id).map(|seat| Seated { id, seat });

        actor.and_then(|actor| self.log.shard(actor.seat.number()).get(actor)).into_iter().flatten()
    }

    /// The number of the last step run, from one. Zero before the first.
    #[must_use]
    pub const fn step_number(&self) -> u64 {
        self.step
    }

    /// Whether a step would run no turn: no actor has anything to react to, and none has a closing turn to run.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        let has_items = self.streams.shards().any(stream::Shard::has_items);
        let has_ended = self.lives.shards().any(lives::Shard::has_ended);

        !has_items && !has_ended
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

        // Mail sent between steps is read in the next step, with the mail the last step's turns sent. The root and the
        // children its `begin` stands up take the empty seats first, then new ones.
        let parity = parity(self.step);
        let seated = self.seats.len();
        let mut seats = Seats::new(mem::take(&mut self.empty), new_seat(seated), 1);
        let me = Seated { id, seat: seats.take() };
        let mut asked = Asked::writing(self.arena(parity, ROOTS), seats);
        let began = A::begin(config, &mut Beginning::new(me, path.clone(), &self.table, &mut asked));
        let (asked, stood, arena, mut seats) = asked.into_parts();
        self.arenas[parity][ROOTS] = arena;
        let root = match began {
            Ok(root) => root,
            Err(fault) => {
                seats.give_back(me.seat);
                for child in stood {
                    seats.give_back(child.seated().seat);
                }
                self.take_back(seated, seats);

                return Err(Refused::Began(fault));
            }
        };
        self.take_back(seated, seats);
        let from = Code::Linked(A::NAME);
        let made = match segment.key() {
            None => What::Root { from },
            Some(key) => What::RootInstanced { from, key: key.clone() },
        };
        let effects: Vec<Effect> = iter::once(Effect::new(me, made)).chain(asked).collect();
        let context = Context { cause: None, root: MailId { sender: id, count: 0 }, depth: 1 };

        self.seat(me, path, Box::new(root));
        let applied = begun(&effects).into_owned();
        self.fold_turn(&applied, stood, context);
        self.streams.fold_owned(applied, &context);
        self.streams.settle();
        self.free_new(seated);

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
        // Every actor with items, in the order of the seats, and every actor whose closing turn is to run. Items for
        // an actor that is not live are never handed to it, and go with the rest at the next step: one never made,
        // one gone, or one that has ended, which runs its closing turn.
        let runs: Vec<(u64, u64, Range<usize>)> = self.streams.shards_mut().flat_map(stream::Shard::hand).collect();
        let ending: Vec<Seated> = self.lives.shards_mut().flat_map(lives::Shard::take_ended).collect();
        for (seat, bits, range) in runs {
            let me = Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) };
            let is_live = self.lives.shard(seat).life(me).is_some_and(lives::Life::is_live);
            if is_live {
                self.job(me, Work::Items(range), &mut jobs, &mut costs);
            }
        }
        for me in ending {
            self.job(me, Work::Closing, &mut jobs, &mut costs);
        }

        let work = costs.iter().sum();
        let mut planned = plan::plan(jobs, &costs, self.recent, workers);
        self.recent = toward(self.recent, work);

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer. Each worker
        // gives the children its turns spawn a share of the empty seats, then new numbers of its own: every
        // `running`th from its place, so no two workers give the same.
        let parity = parity(self.step);
        for arena in &mut self.arenas[parity] {
            arena.clear();
        }
        let running = planned.shares_mut().count();
        let first_new = new_seat(self.seats.len());
        let mut empty = mem::take(&mut self.empty);
        let each = empty.len().div_ceil(running.max(1));
        for ((share, slot), place) in planned.shares_mut().zip(ROOTS + 1..).zip(0..) {
            let arena = self.arena(parity, slot);
            while self.lists.len() <= slot {
                let lists = self.lists();
                self.lists.push(lists);
            }
            let mut lists = mem::take(&mut self.lists[slot]);
            let free = empty.split_off(empty.len().saturating_sub(each));
            lists.seats = Seats::new(free, first_new + place, new_seat(running));
            share.write_to(arena, lists);
        }

        planned
    }

    /// The engine as the step's workers read it while they run their shares.
    #[must_use]
    pub fn reading(&self) -> Reading<'_> {
        let mail = &self.arenas[parity(self.step + 1)];

        Reading {
            table: &self.table,
            streams: &self.streams,
            lives: &self.lives,
            seats: &self.seats,
            mail,
            read_limit: self.read_limit,
        }
    }

    /// Ends a step: puts back the actors the workers ran, takes in how long their turns took, and applies what
    /// the turns asked for (ADR-0011).
    ///
    /// Each view folds a turn by itself, reading no other, and comes out the same in whatever order the turns come,
    /// so each worker's turns are folded as they are. Once every turn is folded the streams settle, pairing the
    /// step's watches and endings. An actor whose closing turn ran leaves last.
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply(&mut self, done: Vec<Done>) {
        let parity = parity(self.step);
        let seated = self.seats.len();
        let mut gone = Vec::new();
        let mut rows = Vec::with_capacity(done.len());
        for Done { mut lists, tally, arena } in done {
            self.costs.merge(tally);
            let slot = arena.slot();
            self.arenas[parity][slot] = arena;
            for Turned { seat, kept } in lists.actors.drain(..) {
                match kept {
                    Kept::Live(actor) => self.actors[seat.index()] = Held::Here(actor),
                    Kept::Gone => gone.push(seat),
                }
            }
            for child in lists.stood.drain(..) {
                self.seat(child.seated(), child.path().clone(), child.into_actor());
            }
            let seats = mem::take(&mut lists.seats);
            self.take_back(seated, seats);
            rows.push((slot, lists));
        }
        self.free_new(seated);

        // Each part of each view takes its column: the bucket every worker filled for it.
        self.table.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.table));
        self.log.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.log));
        self.lives.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.lives));
        self.streams.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.streams));
        self.streams.settle();
        for (slot, lists) in rows {
            self.lists[slot] = lists;
        }

        // Whatever reaches an actor that has gone is dropped when the engine would hand it over.
        for seat in gone {
            self.actors[seat.index()] = Held::Away;
            self.seats[seat.index()] = Seat::Empty;
            self.empty.push(seat);
        }
    }

    /// Takes the actor `me` names out of its seat to run `work`, adding its job and its expected cost. Nothing when
    /// another actor sits there now.
    fn job(&mut self, me: Seated, work: Work, jobs: &mut Vec<Job>, costs: &mut Vec<u64>) {
        let Some(Seat::Taken(sitting)) = self.seats.get(me.seat.index()) else {
            return;
        };
        if sitting.id != me.id {
            return;
        }
        let kind = sitting.kind;
        let Held::Here(actor) = mem::replace(&mut self.actors[me.seat.index()], Held::Away) else {
            return;
        };

        costs.push(self.expect(me.seat, kind, &work));
        jobs.push(Job { id: me.id, seat: me.seat, kind, actor, work });
    }

    /// What the turns of the job of the actor in `seat`, of kind `kind`, that runs `work` are expected to cost, in
    /// nanoseconds.
    fn expect(&self, seat: SeatNumber, kind: KindIndex, work: &Work) -> u64 {
        let key = |item| CostKey { actor: kind, item };
        match work {
            Work::Closing => self.costs.expect(&key(ItemKind::Closing)),
            Work::Items(range) => {
                let items = self.streams.shard(seat.number()).handed(range.clone());

                items.map(|pending| self.costs.expect(&key(item_kind(pending)))).sum()
            }
        }
    }

    /// Folds one turn's effects, emitted in `context`, into every view but the streams, and seats the children
    /// it stood up.
    fn fold_turn(&mut self, effects: &[Effect], stood: Vec<Stood>, context: Context) {
        self.table.fold(effects, &context);
        self.log.fold(effects, &context);
        self.lives.fold(effects, &context);

        for child in stood {
            self.seat(child.seated(), child.path().clone(), child.into_actor());
        }
    }

    /// Empty lists for a worker, with a row of buckets for each view.
    fn lists(&self) -> Lists {
        let routes = Box::new(Routes {
            table: self.table.routed(),
            log: self.log.routed(),
            lives: self.lives.routed(),
            streams: self.streams.routed(),
        });

        Lists { routes, ..Lists::default() }
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

    /// Seats the actor `me` names, at `path`, in the seat it was given. It is run from the next step.
    fn seat(&mut self, me: Seated, path: ActorPath, actor: Box<dyn Runs>) {
        let kind = self.kinds.number(path.last().kind());
        self.reach(me.seat);
        self.seats[me.seat.index()] = Seat::Taken(Sitting { id: me.id, path, kind });
        self.actors[me.seat.index()] = Held::Here(actor);
    }

    /// Adds empty seats up to `seat`, when there are fewer.
    fn reach(&mut self, seat: SeatNumber) {
        while self.seats.len() <= seat.index() {
            self.seats.push(Seat::Empty);
            self.actors.push(Held::Away);
        }
    }

    /// Takes back the seats a writer did not give out, when there were `seated` seats as it began: an empty seat it
    /// was given is empty again, and a new number it gave back is added, to be freed with the rest of the new ones.
    fn take_back(&mut self, seated: usize, seats: Seats) {
        for seat in seats.into_free() {
            let is_new = seat.index() >= seated;
            if is_new {
                self.reach(seat);
            } else {
                self.empty.push(seat);
            }
        }
    }

    /// Frees the seats added since there were `seated` that no actor took: new numbers a writer passed over, or gave
    /// out to a child that never began.
    fn free_new(&mut self, seated: usize) {
        let added = seated..self.seats.len();
        let passed = added.filter(|index| matches!(self.seats[*index], Seat::Empty));
        self.empty.extend(passed.map(new_seat).map(SeatNumber::new));
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: each actor's closing turn when it has ended, and otherwise one turn for
    /// each item it was given, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was given behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let mut tally = Tally::default();
        let (jobs, mut arena, mut lists) = share.into_parts();
        for job in jobs {
            self.turns(job, clock, &mut tally, &mut arena, &mut lists);
        }

        Done { lists, tally, arena }
    }

    /// The bytes of a piece of mail sent in the last step.
    fn bytes<'b>(&'b self, payload: &'b Payload) -> &'b [u8] {
        match payload {
            Payload::InArena(span) => self.mail.get(span.slot).and_then(|arena| arena.read(*span)).unwrap_or_default(),
            Payload::Owned(bytes) => bytes,
        }
    }

    /// The turns of one actor's job, each measured into `tally`, their mail written to `arena`, and their effects,
    /// the turns, and the actor added to `lists`.
    fn turns(&self, job: Job, clock: &impl Clock, tally: &mut Tally, arena: &mut Arena, lists: &mut Lists) {
        let Job { id, seat: number, kind, actor, work } = job;
        let me = Seated { id, seat: number };
        let life = self.lives.shard(number.number()).life(me);
        let (Seat::Taken(seat), Some(life)) = (&self.seats[number.index()], life) else {
            lists.actors.push(Turned { seat: number, kept: Kept::Live(actor) });
            return;
        };
        let key = |item| CostKey { actor: kind, item };

        match (work, life.state()) {
            (Work::Closing, State::Ending { how, context }) => {
                let begins = lists.effects.len();
                let mut asked =
                    Asked::continuing(mem::take(&mut lists.effects), mem::take(arena), mem::take(&mut lists.seats));
                let why = why(how);
                let start = clock.nanos();
                let ended =
                    actor.end(&mut Ending::new(me, seat.path.clone(), self.table, &mut asked, life.sent(), why));
                tally.add(key(ItemKind::Closing), clock.nanos().saturating_sub(start));
                let (effects, _, written, seats) = asked.into_parts();
                lists.effects = effects;
                lists.seats = seats;
                *arena = written;
                // A closing turn that fails asks for nothing: only its log lines are kept, and the engine adds its
                // fault to them (ADR-0009). Either way the engine adds that the actor's life is over, after what the
                // turn sent.
                if let Err(fault) = ended {
                    keep_lines(&mut lists.effects, begins);
                    lists.effects.push(Effect::new(me, What::EndFailed(fault)));
                }
                lists.effects.push(Effect::new(me, What::Ended(how.clone())));

                lists.route(Ran { effects: begins..lists.effects.len(), stood: Vec::new(), context: *context });
                lists.actors.push(Turned { seat: number, kept: Kept::Gone });
            }
            (Work::Items(range), State::Live) => {
                let mut actor = actor;
                let mut stood_since = ByActor::default();
                let mut count = life.sent();
                for pending in self.streams.shard(number.number()).handed(range) {
                    let table = Since { table: self.table, stood: &stood_since };
                    let begins = lists.effects.len();
                    let mut asked =
                        Asked::continuing(mem::take(&mut lists.effects), mem::take(arena), mem::take(&mut lists.seats));
                    let source = pending.source();
                    let (item, context) = match pending {
                        Pending::Mail(envelope) => {
                            let Envelope { id: cause, kind, bytes, root, depth, .. } = envelope;
                            let bytes = self.bytes(bytes);
                            let mail = Mail::new(*kind, bytes, source, *root, *depth, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(*cause), root: *root, depth: depth + 1 })
                        }
                        Pending::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(me, seat.path.clone(), &table, &mut asked, count, source);
                    let start = clock.nanos();
                    let handled = actor.handle(item, &mut turn);
                    tally.add(key(item_kind(pending)), clock.nanos().saturating_sub(start));
                    let ran = Ran::of(me, handled, asked, context, begins, lists, arena);

                    let effects = &lists.effects[ran.effects.clone()];
                    count = sent(effects, id).unwrap_or(count);
                    let stood =
                        ran.stood.iter().map(|child| (child.id().bits(), (child.path().clone(), child.seated().seat)));
                    stood_since.extend(stood);
                    let over = ends(effects, id);
                    lists.route(ran);
                    if over {
                        break;
                    }
                }

                lists.actors.push(Turned { seat: number, kept: Kept::Live(actor) });
            }
            // A plan gives a closing turn only to an actor that has ended, and items only to a live one.
            _ => lists.actors.push(Turned { seat: number, kept: Kept::Live(actor) }),
        }
    }
}

impl Lists {
    /// Routes a finished turn's effects into this worker's row of buckets for each view, taking them out of the
    /// list, and keeps the children the turn stood up, to be seated.
    ///
    /// Every view but the streams borrows the effects; the streams then take them by value. A turn that stood up a
    /// child whose `begin` failed is routed without what that child left.
    fn route(&mut self, ran: Ran) {
        let Ran { effects, stood, context } = ran;
        let routes = &mut self.routes;
        let is_replaced = match begun(&self.effects[effects.clone()]) {
            Cow::Borrowed(applied) => {
                routes.route(applied, &context);
                false
            }
            Cow::Owned(applied) => {
                routes.route(&applied, &context);
                routes.streams.route_owned(applied, &context);
                true
            }
        };
        let these = self.effects.drain(effects.start..);
        if !is_replaced {
            routes.streams.route_owned(these, &context);
        }

        self.stood.extend(stood);
    }
}

impl Routes {
    /// Routes one turn's effects, emitted in `context`, into every view's bucket but the streams'.
    fn route(&mut self, effects: &[Effect], context: &Context) {
        self.table.route(effects, context);
        self.log.route(effects, context);
        self.lives.route(effects, context);
    }
}

impl Ran {
    /// The turn of the actor `me` names that returned `outcome`, having asked for `asked`, in `context`, its effects
    /// added to `lists` from `begins` on and its mail written to `arena`, both given back.
    ///
    /// A turn that fails changes nothing outside its actor: what it asked for is dropped but for its log lines,
    /// and the engine writes the one effect that says the actor ended (ADR-0009). The seats of the children it
    /// stood up are given back.
    fn of(
        me: Seated,
        outcome: Result<(), Fault>,
        asked: Asked,
        context: Context,
        begins: usize,
        lists: &mut Lists,
        arena: &mut Arena,
    ) -> Self {
        let (effects, mut stood, written, seats) = asked.into_parts();
        lists.effects = effects;
        lists.seats = seats;
        *arena = written;
        if let Err(fault) = outcome {
            keep_lines(&mut lists.effects, begins);
            lists.effects.push(Effect::new(me, What::Failed(fault)));
            for child in mem::take(&mut stood) {
                lists.seats.give_back(child.seated().seat);
            }
        }

        Self { effects: begins..lists.effects.len(), stood, context }
    }
}

/// Whether `effects` end the actor `id` stands for.
fn ends(effects: &[Effect], id: ActorId) -> bool {
    effects.iter().any(|effect| ending(effect, id).is_some())
}

/// The count of the last mail the actor `id` stands for sent among `effects`, or none when it sent none.
fn sent(effects: &[Effect], id: ActorId) -> Option<u64> {
    effects
        .iter()
        .filter_map(|effect| match effect.what() {
            What::Send { id: mail, .. } if mail.sender == id => Some(mail.count),
            _ => None,
        })
        .max()
}

/// Drops every effect from `begins` on but the logged lines: what is kept of a turn that failed (ADR-0009).
fn keep_lines(effects: &mut Vec<Effect>, begins: usize) {
    let lines: Vec<Effect> = effects.drain(begins..).filter(is_line).collect();
    effects.extend(lines);
}

/// What a pending item is, as far as the cost of reacting to it goes.
const fn item_kind(pending: &Pending) -> ItemKind {
    match pending {
        Pending::Mail(envelope) => ItemKind::Mail(envelope.kind),
        Pending::Ended { .. } => ItemKind::Ended,
    }
}

/// The number of the seat at `index` in the list of seats.
const fn new_seat(index: usize) -> u64 {
    index as u64
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
