//! The engine: it runs turns in steps, and applies what they asked for (ADR-0009, ADR-0011).

mod begun;
mod costs;
mod fold;
mod plan;
mod since;
#[cfg(test)]
mod tests;

pub use costs::Tally;
pub use fold::{Fold, Folding};
pub use plan::{Plan, Share, Workers};

use crate::id::root_id;
use crate::stream::{self, Handed, Streams};
use crate::table::Actors;
use crate::{
    ActorTable, ByActor, BySeat, Clock, Context, Entry, Envelope, Folded, Lives, Log, Part, Routed, State, lives,
    rounded,
};
use aethereal_actor::{
    Actor, Arena, Asked, Beginning, Code, Effect, Ending, Fault, How, Item, Mail, MailId, Payload, Refused, Runs,
    SeatNumber, Seated, Seats, Stood, Turn, What, Why,
};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use begun::begun;
use costs::{CostKey, Costs, ItemKind, KindIndex, Kinds, toward};
use plan::Packing;
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
    /// The seats of the actors the engine still runs, the live ones and the ended ones whose closing turn is still
    /// to run, in parts as the views keep them: part `p` holds every seat numbered `p` more than a multiple of the
    /// number of parts. An actor leaves its seat when its closing turn takes it.
    seating: Vec<Seating>,
    /// The actors themselves, in the same parts, kept apart from their seats, which the workers read while they run.
    actors: Vec<BySeat<Held>>,
    /// What the turns waiting in each part are expected to cost, in nanoseconds, as of the last fold.
    expected: Vec<u64>,
    /// What planning and gathering a step pack parts in.
    packing: Packing,
    /// What the steps hand out, emptied and kept from one step to the next for its room, so a step allocates none of
    /// it again.
    spare: Spare,
    /// Where mail is written: for each parity of step, an arena for each writer, the first for mail sent between
    /// steps.
    arenas: [Vec<Arena>; 2],
    /// What each worker fills in a step, by writer, emptied and kept from one step to the next for its room.
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
    /// The host's quota for the bytes of mail in flight, shared equally among the arenas of every writer.
    mail_bytes: usize,
}

/// One part of the engine's seats: the actors in them, and the seats it gives out next.
struct Seating {
    /// Each actor in a seat of this part.
    seated: BySeat<Sitting>,
    /// The part's empty seats, given out before new numbers are.
    free: Vec<SeatNumber>,
    /// The number of the part's next new seat.
    next: u64,
}

/// An actor in its seat: what is fixed when it is seated.
struct Sitting {
    /// Where the actor stands.
    path: ActorPath,
    /// Its kind, as the engine numbers it.
    kind: KindIndex,
}

/// Whether the engine holds the actor of a seat.
enum Held {
    /// It does.
    Here(Box<dyn Runs>),
    /// It does not: the actor is out with a worker for the step.
    Away,
}

/// The engine as a step's workers read it while they run: the table, and the mail the last step sent (ADR-0011).
///
/// What a worker reads of its own actors, their seats, lives, and items, it
/// owns for the step, in the parts it was given.
pub struct Reading<'a> {
    table: &'a ActorTable,
    /// The arenas the last step's mail was written to, by writer.
    mail: &'a [Arena],
    read_limit: usize,
}

/// What a step hands out and takes back, kept empty between steps for its room.
#[derive(Default)]
struct Spare {
    /// Lists of the parts a worker runs.
    parcels: Vec<Vec<Parcel>>,
    /// Lists of the parts a worker folds.
    folds: Vec<Vec<fold::Pile>>,
    /// Parts to fold, each with a column of every view, emptied.
    piles: Vec<fold::Pile>,
    /// What the folded parts' streams hand on to other parts.
    forward: Vec<(u64, stream::Record)>,
}

/// One part of the engine as a worker owns it for a step: what has arrived for the actors seated in it, their lives,
/// their seats, and the actors themselves (ADR-0011).
///
/// A turn reads and changes only its own actor's part, so the parts a step
/// runs are taken out of the engine, each by one worker, and put back when it
/// is applied.
pub struct Parcel {
    /// Which part it is.
    part: usize,
    streams: stream::Shard,
    lives: lives::Shard,
    seating: Seating,
    actors: BySeat<Held>,
}

/// What one worker's share of a step left: its turns and how long they took, and the parts it ran.
pub struct Done {
    lists: Lists,
    /// The arena the worker wrote its mail to, to keep for the step after next.
    arena: Arena,
    parcels: Vec<Parcel>,
}

/// What a worker fills in a step: its row of buckets for each view, the children its turns stood up, and each actor
/// it ran, and the seats it gives the children. The engine empties it and hands it back the next step, so nothing in
/// it is allocated again.
#[derive(Default)]
pub struct Lists {
    /// The effects of the turn running, routed into the buckets when it finishes.
    effects: Vec<Effect>,
    routes: Routes,
    stood: Vec<Stood>,
    seats: Seats,
    /// How long the turns took, by what they were.
    tally: Tally,
    /// Each actor with items in the part running, as it was handed them: kept for its room.
    runs: Vec<(u64, u64, Range<usize>)>,
}

/// One writer's row of buckets for each view.
#[derive(Default)]
struct Routes {
    table: Routed<Actors>,
    log: Routed<Log>,
    lives: Routed<Lives>,
    streams: Routed<Streams>,
}

/// What an actor runs in a step.
enum Work {
    /// One turn for each item its part of the streams handed out in this range, in order.
    Items(Range<usize>),
    /// Its closing turn.
    Closing,
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
    /// An engine with no actors, its views in `parts` parts [`rounded`] up to a power of two, whose turns read at
    /// most `read_limit` bytes into any one piece of mail, and whose mail in flight is written to `mail_bytes` of
    /// arenas, shared equally among the writers, two arenas each.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize, mail_bytes: usize) -> Self {
        let parts = rounded(parts);

        Self {
            table: ActorTable::new(parts),
            streams: Folded::new(parts),
            log: Folded::new(parts),
            lives: Folded::new(parts),
            seating: (0..parts.get())
                .map(|part| Seating { seated: BySeat::empty(parts), free: Vec::new(), next: new_seat(part) })
                .collect(),
            actors: (0..parts.get()).map(|_| BySeat::empty(parts)).collect(),
            expected: vec![0; parts.get()],
            packing: Packing::default(),
            spare: Spare::default(),
            arenas: [Vec::new(), Vec::new()],
            lists: Vec::new(),
            kinds: Kinds::new(),
            costs: Costs::new(),
            recent: 0,
            step: 0,
            read_limit,
            mail_bytes,
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
        // children its `begin` stands up take the empty seats first, then new ones, in any part.
        let parity = parity(self.step);
        let mut seats = Seats::default();
        self.seats(0..self.seating.len(), &mut seats);
        let me = Seated { id, seat: seats.take() };
        let mut asked = Asked::writing(self.arena(parity, ROOTS, 0), seats);
        let began = A::begin(config, &mut Beginning::new(me, &path, &self.table, &mut asked));
        let (asked, stood, arena, mut seats) = asked.into_parts();
        self.arenas[parity][ROOTS] = arena;
        let root = match began {
            Ok(root) => root,
            Err(fault) => {
                seats.give_back(me.seat);
                for child in stood {
                    seats.give_back(child.seated().seat);
                }
                self.take_back(&mut seats);

                return Err(Refused::Began(fault));
            }
        };
        self.take_back(&mut seats);
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
        self.lives.fold_records(senders(&applied).into_iter().map(|(actor, count)| lives::sent(actor, count)));
        self.streams.fold_owned(applied, &context);
        self.streams.settle();
        for part in 0..self.seating.len() {
            self.expected[part] = self.expect(part);
        }

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

    /// Begins a step on `workers`: takes each part of the engine with something to run out of it, with the actors in
    /// the part and what they react to, and shares the parts among the workers that are to run, by what their turns
    /// are expected to cost (ADR-0011).
    pub fn plan(&mut self, workers: Workers) -> Plan {
        self.step += 1;

        self.packing.parts.clear();
        for part in 0..self.seating.len() {
            if self.has_work(part) {
                self.packing.parts.push((part, self.expected[part]));
            }
        }
        let work = self.packing.parts.iter().map(|(_, cost)| cost).sum();
        let (delta, running) = plan::plan(&self.packing.parts, self.recent, workers);
        self.recent = toward(self.recent, work);
        let running = running.max(1);
        self.packing.pack(running);

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer, each given an
        // equal share of the host's quota for mail. Each worker seats the children its turns spawn in every
        // `running`th part from its place, of all the parts and not only the ones it runs, so children spread across
        // the parts and no two workers give the same seat.
        let parity = parity(self.step);
        let share = self.mail_bytes / (2 * (ROOTS + 1 + workers.max.max(1)));
        for arena in &mut self.arenas[parity] {
            arena.clear(share);
        }
        let mut shares = Vec::with_capacity(running);
        for place in 0..running {
            let slot = ROOTS + 1 + place;
            let arena = self.arena(parity, slot, share);
            while self.lists.len() <= slot {
                let lists = self.lists();
                self.lists.push(lists);
            }
            // What is left in its place allocates nothing, and the lists come back to the same place, so each stays
            // with the worker that fills it.
            let mut lists = mem::take(&mut self.lists[slot]);
            self.seats((place..self.seating.len()).step_by(running), &mut lists.seats);
            let mut parcels = self.spare.parcels.pop().unwrap_or_default();
            for index in 0..self.packing.shares[place].1.len() {
                let part = self.packing.shares[place].1[index];
                parcels.push(self.parcel(part));
            }
            shares.push(Share { parcels, arena, lists });
        }

        Plan { delta, shares }
    }

    /// The engine as the step's workers read it while they run their shares.
    #[must_use]
    pub fn reading(&self) -> Reading<'_> {
        let mail = &self.arenas[parity(self.step + 1)];

        Reading { table: &self.table, mail, read_limit: self.read_limit }
    }

    /// Ends a step on the thread that calls it: gathers what the workers left, folds every part, and settles
    /// (ADR-0011). [`Engine::gather`], [`Folding::fold`] on each fold, and [`Engine::settle`] do the same with the
    /// folds shared among workers.
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply(&mut self, done: Vec<Done>) {
        let folds = self.gather(done.into_iter());
        let folding = self.folding();
        let folded: Vec<Fold> = folds.into_iter().map(|fold| folding.fold(fold)).collect();

        self.settle(folded);
    }

    /// Begins the end of a step: puts back the parts the workers ran, takes in how long their turns took, seats the
    /// children the turns stood up, and takes out each part some worker routed a record to, with every worker's
    /// records for it, shared among as many folds as there were workers by how many records each holds. A part no
    /// record reached is not touched.
    pub fn gather(&mut self, done: impl ExactSizeIterator<Item = Done>) -> Vec<Fold> {
        let parity = parity(self.step);
        let running = done.len();
        for Done { mut lists, arena, mut parcels } in done {
            self.costs.merge(&mut lists.tally);
            let slot = arena.slot();
            self.arenas[parity][slot] = arena;
            while let Some(parcel) = parcels.pop() {
                self.put(parcel);
            }
            self.spare.parcels.push(parcels);
            self.take_back(&mut lists.seats);
            let mut stood = mem::take(&mut lists.stood);
            while let Some(child) = stood.pop() {
                self.seat(child.seated(), child.path().clone(), child.into_actor());
            }
            lists.stood = stood;
            self.lists[slot] = lists;
        }

        self.packing.parts.clear();
        for part in 0..self.seating.len() {
            let records: usize = self.lists.iter().map(|lists| lists.routes.records(part)).sum();
            if records > 0 {
                self.packing.parts.push((part, u64::try_from(records).unwrap_or(u64::MAX)));
            }
        }
        let folding = running.min(self.packing.parts.len());
        if folding == 0 {
            return Vec::new();
        }
        self.packing.pack(folding);

        let mut folds = Vec::with_capacity(folding);
        for index in 0..folding {
            let mut piles = self.spare.folds.pop().unwrap_or_default();
            for place in 0..self.packing.shares[index].1.len() {
                let part = self.packing.shares[index].1[place];
                piles.push(self.pile(part));
            }
            folds.push(Fold { parts: piles });
        }

        folds
    }

    /// The engine as the step's workers read it while they fold.
    #[must_use]
    pub fn folding(&self) -> Folding<'_> {
        Folding { costs: &self.costs }
    }

    /// Ends a step: puts back every part the workers folded, and folds what their streams handed on to other parts:
    /// the endings for the actors that watch.
    pub fn settle(&mut self, folds: impl IntoIterator<Item = Fold>) {
        let mut forward = mem::take(&mut self.spare.forward);
        for Fold { parts: mut piles } in folds {
            while let Some(mut pile) = piles.pop() {
                forward.append(&mut pile.forward);
                self.unpile(&mut pile);
                self.spare.piles.push(pile);
            }
            self.spare.folds.push(piles);
        }

        while let Some((key, record)) = forward.pop() {
            let part = self.part(SeatNumber::new(key));
            self.streams.fold_records(iter::once((key, record)));
            self.expected[part] = self.expect(part);
        }
        self.spare.forward = forward;
    }

    /// Whether part `part` has anything to run: an item that has arrived, or a closing turn.
    fn has_work(&self, part: usize) -> bool {
        self.streams.at(part).has_items() || self.lives.at(part).has_ended()
    }

    /// What the turns waiting in part `part` are expected to cost, in nanoseconds.
    fn expect(&self, part: usize) -> u64 {
        fold::expected(self.streams.at(part), self.lives.at(part), &self.seating[part].seated, &self.costs)
    }

    /// Takes part `part` of every view out, with every worker's records for it, to fold, into a spare pile.
    fn pile(&mut self, part: usize) -> fold::Pile {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let mut pile = self.spare.piles.pop().unwrap_or_else(|| fold::Pile::empty(parts));
        let empty = Seating { seated: BySeat::empty(parts), free: Vec::new(), next: new_seat(part) };
        let rows = &mut self.lists;
        self.table.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.table), &mut pile.table);
        self.log.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.log), &mut pile.log);
        self.lives.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.lives), &mut pile.lives);
        self.streams.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.streams), &mut pile.streams);
        pile.seating = mem::replace(&mut self.seating[part], empty);
        pile.part = part;
        pile.expected = 0;

        pile
    }

    /// Puts back a folded part from `pile`, leaving it empty but for what its streams handed on.
    fn unpile(&mut self, pile: &mut fold::Pile) {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let part = pile.part;
        let rows = &mut self.lists;
        self.table.empty(part, &mut pile.table, rows.iter_mut().map(|lists| &mut lists.routes.table));
        self.log.empty(part, &mut pile.log, rows.iter_mut().map(|lists| &mut lists.routes.log));
        self.lives.empty(part, &mut pile.lives, rows.iter_mut().map(|lists| &mut lists.routes.lives));
        self.streams.empty(part, &mut pile.streams, rows.iter_mut().map(|lists| &mut lists.routes.streams));
        let empty = Seating { seated: BySeat::empty(parts), free: Vec::new(), next: new_seat(part) };
        self.seating[part] = mem::replace(&mut pile.seating, empty);
        self.expected[part] = pile.expected;
    }

    /// Takes part `part` out of the engine for a worker to run.
    fn parcel(&mut self, part: usize) -> Parcel {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let empty = Seating { seated: BySeat::empty(parts), free: Vec::new(), next: new_seat(part) };

        Parcel {
            part,
            streams: self.streams.take(part),
            lives: self.lives.take(part),
            seating: mem::replace(&mut self.seating[part], empty),
            actors: mem::replace(&mut self.actors[part], BySeat::empty(parts)),
        }
    }

    /// Puts back a part a worker ran.
    fn put(&mut self, parcel: Parcel) {
        let Parcel { part, streams, lives, seating, actors } = parcel;
        self.streams.put(part, streams);
        self.lives.put(part, lives);
        self.seating[part] = seating;
        self.actors[part] = actors;
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
        let routes = Routes {
            table: self.table.routed(),
            log: self.log.routed(),
            lives: self.lives.routed(),
            streams: self.streams.routed(),
        };

        Lists { routes, ..Lists::default() }
    }

    /// The arena of the writer numbered `slot` for steps of `parity`, taken out to write to; made when it is the
    /// first, its slab given `bytes`.
    fn arena(&mut self, parity: usize, slot: usize, bytes: usize) -> Arena {
        let arenas = &mut self.arenas[parity];
        while arenas.len() <= slot {
            let mut arena = Arena::new(arenas.len());
            arena.clear(bytes);
            arenas.push(arena);
        }

        mem::replace(&mut arenas[slot], Arena::new(slot))
    }

    /// Seats the actor `me` names, at `path`, in the seat it was given. It is run from the next step.
    fn seat(&mut self, me: Seated, path: ActorPath, actor: Box<dyn Runs>) {
        let kind = self.kinds.number(path.last().kind());
        let part = self.part(me.seat);
        self.seating[part].seated.insert(me, Sitting { path, kind });
        self.actors[part].insert(me, Held::Here(actor));
    }

    /// The part `seat` falls in.
    fn part(&self, seat: SeatNumber) -> usize {
        part(seat, self.seating.len())
    }

    /// Gives `seats` the seats a writer gives out in the parts `parts`: their free seats, then their new numbers.
    fn seats(&mut self, parts: impl Iterator<Item = usize>, seats: &mut Seats) {
        seats.clear(new_seat(self.seating.len()));
        for part in parts {
            let seating = &mut self.seating[part];
            seats.give(seating.free.drain(..), seating.next);
        }
    }

    /// Takes back the seats a writer did not give out, into the parts they fall in, and where each part's new
    /// numbers have got to, leaving `seats` empty.
    fn take_back(&mut self, seats: &mut Seats) {
        let parts = self.seating.len();
        let (free, next) = seats.drain();
        for seat in free {
            self.seating[part(seat, parts)].free.push(seat);
        }
        for number in next {
            self.seating[part(SeatNumber::new(number), parts)].next = number;
        }
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: in each of its parts, one turn for each item an actor there was handed, and
    /// each closing turn to run, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was handed behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let Share { mut parcels, mut arena, mut lists } = share;
        let mut tally = mem::take(&mut lists.tally);
        for parcel in &mut parcels {
            self.run_part(parcel, clock, &mut tally, &mut arena, &mut lists);
        }
        lists.tally = tally;

        Done { lists, arena, parcels }
    }

    /// Runs one part: hands out what has arrived, then runs every actor with items, in the order of the seats, and
    /// every actor whose closing turn is to run. Items for an actor that is not live are never handed to it: one
    /// never made, one gone, or one that has ended, which runs its closing turn instead.
    fn run_part(
        &self,
        parcel: &mut Parcel,
        clock: &impl Clock,
        tally: &mut Tally,
        arena: &mut Arena,
        lists: &mut Lists,
    ) {
        let mut runs = mem::take(&mut lists.runs);
        runs.extend(parcel.streams.hand());
        for (seat, bits, range) in &runs {
            let me = Seated { id: ActorId::new(*bits), seat: SeatNumber::new(*seat) };
            let is_live = parcel.lives.life(me).is_some_and(lives::Life::is_live);
            if is_live {
                self.turns(parcel, me, Work::Items(range.clone()), clock, tally, arena, lists);
            }
        }
        runs.clear();
        lists.runs = runs;

        for me in parcel.lives.take_ended() {
            self.turns(parcel, me, Work::Closing, clock, tally, arena, lists);
        }
    }

    /// The bytes of a piece of mail sent in the last step.
    fn bytes(&self, payload: Payload) -> &[u8] {
        self.mail.get(payload.slot()).and_then(|arena| arena.read(payload)).unwrap_or_default()
    }

    /// The turns of the actor `me` names in `parcel` that `work` asks for, each measured into `tally`, their mail
    /// written to `arena`, and their effects added to `lists`. The actor is taken out of its seat while its turns
    /// run, and put back, or its seat freed once its closing turn has run. Nothing when another actor sits there.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    fn turns(
        &self,
        parcel: &mut Parcel,
        me: Seated,
        work: Work,
        clock: &impl Clock,
        tally: &mut Tally,
        arena: &mut Arena,
        lists: &mut Lists,
    ) {
        let Parcel { streams, lives, seating, actors, .. } = parcel;
        let (Some(seat), Some(life), Some(held)) = (seating.seated.get(me), lives.life(me), actors.get_mut(me)) else {
            return;
        };
        let Held::Here(mut actor) = mem::replace(held, Held::Away) else {
            return;
        };
        let key = |item| CostKey { actor: seat.kind, item };

        match (work, life.state()) {
            (Work::Closing, State::Ending(ending)) => {
                let (how, context) = &**ending;
                let begins = lists.effects.len();
                let mut asked =
                    Asked::continuing(mem::take(&mut lists.effects), mem::take(arena), mem::take(&mut lists.seats));
                let why = why(how);
                let mut ending = Ending::new(me, &seat.path, self.table, &mut asked, life.sent(), why);
                let ended = tally.time(key(ItemKind::Closing), clock, || actor.end(&mut ending));
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
                // Whatever reaches an actor that has gone is dropped when the engine would hand it over.
                seating.seated.remove(me);
                seating.free.push(me.seat);
                actors.remove(me);
            }
            (Work::Items(range), State::Live) => {
                let mut stood_since = ByActor::default();
                let sent_before = life.sent();
                let mut count = sent_before;
                for handed in streams.handed(range) {
                    let table = Since { table: self.table, stood: &stood_since };
                    let begins = lists.effects.len();
                    let mut asked =
                        Asked::continuing(mem::take(&mut lists.effects), mem::take(arena), mem::take(&mut lists.seats));
                    let source = handed.source();
                    let (item, context) = match handed {
                        Handed::Mail { id: cause, envelope: &Envelope { kind, bytes, root, depth, .. } } => {
                            let bytes = self.bytes(bytes);
                            let mail = Mail::new(kind, bytes, source, root, depth, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(cause), root, depth: depth + 1 })
                        }
                        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(me, &seat.path, &table, &mut asked, count, source);
                    let handled = tally.time(key(item_kind(handed)), clock, || actor.handle(item, &mut turn));
                    let ran = Ran::of(me, handled, asked, context, begins, lists, arena);

                    let effects = &lists.effects[ran.effects.clone()];
                    count = sent(effects, me.id).unwrap_or(count);
                    if !ran.stood.is_empty() {
                        for (child, sent) in senders(effects).into_iter().filter(|(actor, _)| actor.id != me.id) {
                            let (key, record) = lives::sent(child, sent);
                            lists.routes.lives.put(key, record);
                        }
                    }
                    let stood =
                        ran.stood.iter().map(|child| (child.id().bits(), (child.path().clone(), child.seated().seat)));
                    stood_since.extend(stood);
                    let over = ends(effects, me.id);
                    lists.route(ran);
                    if over {
                        break;
                    }
                }

                if count > sent_before {
                    let (key, record) = lives::sent(me, count);
                    lists.routes.lives.put(key, record);
                }
                put_back(actors, me, actor);
            }
            // A plan gives a closing turn only to an actor that has ended, and items only to a live one.
            _ => put_back(actors, me, actor),
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
    /// How many records this row holds for part `part`, across the views.
    fn records(&self, part: usize) -> usize {
        self.table.records(part) + self.log.records(part) + self.lives.records(part) + self.streams.records(part)
    }

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

/// Puts the actor `me` names back in its seat in `actors` after its turns.
fn put_back(actors: &mut BySeat<Held>, me: Seated, actor: Box<dyn Runs>) {
    if let Some(held) = actors.get_mut(me) {
        *held = Held::Here(actor);
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
        .filter(|effect| effect.by() == id)
        .filter_map(|effect| match effect.what() {
            What::Send { count, .. } => Some(*count),
            _ => None,
        })
        .max()
}

/// Each actor that sent mail among `effects`, with the count of the last piece it sent.
fn senders(effects: &[Effect]) -> Vec<(Seated, u64)> {
    let mut senders: Vec<(Seated, u64)> = Vec::new();
    for effect in effects {
        let What::Send { count, .. } = effect.what() else {
            continue;
        };
        let by = effect.seated();
        match senders.iter_mut().find(|(actor, _)| *actor == by) {
            Some((_, last)) => *last = (*last).max(*count),
            None => senders.push((by, *count)),
        }
    }

    senders
}

/// Drops every effect from `begins` on but the logged lines: what is kept of a turn that failed (ADR-0009).
fn keep_lines(effects: &mut Vec<Effect>, begins: usize) {
    let lines: Vec<Effect> = effects.drain(begins..).filter(is_line).collect();
    effects.extend(lines);
}

/// What a pending item is, as far as the cost of reacting to it goes.
const fn item_kind(handed: Handed<'_>) -> ItemKind {
    match handed {
        Handed::Mail { envelope, .. } => ItemKind::Mail(envelope.kind),
        Handed::Ended { .. } => ItemKind::Ended,
    }
}

/// Which of `parts` parts, a power of two, `seat` falls in.
const fn part(seat: SeatNumber, parts: usize) -> usize {
    seat.index() & parts.wrapping_sub(1)
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
