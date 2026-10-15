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
pub use plan::{Plan, Share, Split, Whole, Workers};

use crate::id::root_id;
use crate::stream::{self, Handed, Streams};
use crate::table::Actors;
use crate::watch::{Notice, Watches};
use crate::{
    ActorTable, ByActor, BySeat, Clock, Context, Entry, Envelope, Folded, Lives, Log, Part, Routed, State, lives,
    rounded,
};
use aethereal_actor::{
    Above, Actor, Arena, Asked, Beginning, Code, Effect, Ending, Fault, How, Item, Mail, MailId, Payload, Refused,
    Runs, SeatNumber, Seated, Seats, Stood, Turn, What, Why,
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
    /// Who watches each actor, and which have ended.
    watches: Folded<Watches>,
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
    /// The parts that may have something to run in the next step: each part the last fold reached, and every part once
    /// a root is made. A part with something to run is always among them, so a step looks at no other.
    waiting: Vec<usize>,
    /// The empty seats, in any part, given out before new numbers are.
    free: Vec<SeatNumber>,
    /// The number of each part's next new seat.
    next: Vec<u64>,
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

/// One part of the engine's seats: the actors in them.
struct Seating {
    /// Each actor in a seat of this part.
    seated: BySeat<Sitting>,
}

/// An actor in its seat: what is fixed when it is seated.
struct Sitting {
    /// Where the actor stands.
    path: ActorPath,
    /// What stands above it.
    above: Above,
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
    /// The endings the folded parts' watches hand on to the streams of the actors that watch.
    forward: Vec<Notice>,
    /// The parts some writer routed a record to in a step.
    touched: Vec<usize>,
    /// What each worker of a shared step left, while its parts are put back: empty between.
    left: Vec<(Lists, Arena)>,
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

/// One part of the engine as its turns change it, where it lies: in a parcel a worker owns, or in the engine.
struct Room<'a> {
    streams: &'a mut stream::Shard,
    lives: &'a mut lives::Shard,
    seating: &'a mut Seating,
    actors: &'a mut BySeat<Held>,
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
    /// The seats of the actors whose closing turns ran, empty once the step is applied: no seat is given again in the
    /// step that freed it, while its actor's last records are still to fold.
    freed: Vec<SeatNumber>,
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
    watches: Routed<Watches>,
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
            watches: Folded::new(parts),
            log: Folded::new(parts),
            lives: Folded::new(parts),
            seating: (0..parts.get()).map(|_| Seating { seated: BySeat::empty(parts) }).collect(),
            actors: (0..parts.get()).map(|_| BySeat::empty(parts)).collect(),
            waiting: Vec::new(),
            free: Vec::new(),
            next: (0..parts.get()).map(new_seat).collect(),
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
        !self.waiting.iter().any(|part| self.has_work(*part))
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
        let mut seats = self.lend_seats();
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
                self.take_back_lent(seats);

                return Err(Refused::Began(fault));
            }
        };
        self.take_back_lent(seats);
        let from = Code::Linked(A::NAME);
        let made = match segment.key() {
            None => What::Root { from },
            Some(key) => What::RootInstanced { from, key: key.clone() },
        };
        let effects: Vec<Effect> = iter::once(Effect::new(me, made)).chain(asked).collect();
        let context = Context { cause: None, root: MailId { sender: id, count: 0 }, depth: 1 };

        self.seat(me, path, Above::Chassis, Box::new(root));
        let applied = begun(&effects).into_owned();
        self.fold_turn(&applied, stood, context);
        self.lives.fold_records(senders(&applied).into_iter().map(|(actor, count)| lives::sent(actor, count)));
        self.watches.fold(&applied, &context);
        self.streams.fold_owned(applied, &context);
        let mut forward = mem::take(&mut self.spare.forward);
        for shard in self.watches.shards_mut() {
            shard.settle(&mut forward);
        }
        self.hand_on(forward);
        for part in 0..self.seating.len() {
            self.expected[part] = self.expect(part);
        }
        self.waiting.extend(0..self.seating.len());

        Ok(id)
    }

    /// Runs one step on the thread that calls it: plans it for one worker, runs it where its parts lie, and applies
    /// what it left (ADR-0011).
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn step(&mut self, clock: &impl Clock) {
        match self.plan(Workers::ONE).into_split() {
            Split::Whole(whole) => {
                let left = self.run_whole(whole, clock);
                self.apply_whole(left);
            }
            Split::Shares(shares) => {
                let reading = self.reading();
                let done = shares.into_iter().map(|share| reading.run(share, clock)).collect();
                self.apply(done);
            }
        }
    }

    /// Runs a step planned for one worker on the thread that calls it, each part where it lies: what
    /// [`Reading::run`] does with the parts taken out. [`Engine::apply_whole`] applies what it left.
    pub fn run_whole(&mut self, whole: Whole, clock: &impl Clock) -> Whole {
        let Whole { arena, mut lists } = whole;
        let mut tally = mem::take(&mut lists.tally);
        let mut asked = Asked::continuing(mem::take(&mut lists.effects), arena, mem::take(&mut lists.seats));
        let mail = &self.arenas[parity(self.step + 1)];
        let reading = Reading { table: &self.table, mail, read_limit: self.read_limit };
        for (part, _) in &self.packing.parts {
            let room = Room {
                streams: self.streams.at_mut(*part),
                lives: self.lives.at_mut(*part),
                seating: &mut self.seating[*part],
                actors: &mut self.actors[*part],
            };
            reading.run_part(room, clock, &mut tally, &mut asked, &mut lists);
        }
        let (effects, _, arena, seats) = asked.into_parts();
        lists.effects = effects;
        lists.seats = seats;
        lists.tally = tally;

        Whole { arena, lists }
    }

    /// Ends a step run by [`Engine::run_whole`] on the thread that calls it: takes in what its turns left, and folds
    /// each part some record reached where it lies, then puts the endings its watches hand on in the streams of the
    /// actors that watch.
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply_whole(&mut self, whole: Whole) {
        let Whole { arena, mut lists } = whole;
        self.take_back_lent(mem::take(&mut lists.seats));
        self.take_in(lists, arena);

        let mut forward = mem::take(&mut self.spare.forward);
        let mut touched = self.touched();
        for &part in &touched {
            let rows = &mut self.lists;
            self.table.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.table));
            self.log.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.log));
            self.lives.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.lives));
            self.streams.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.streams));
            self.watches.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.watches));
            self.watches.at_mut(part).settle(&mut forward);
            self.expected[part] = self.expect(part);
            self.waiting.push(part);
        }
        touched.clear();
        self.spare.touched = touched;
        self.hand_on(forward);
    }

    /// Begins a step on `workers`: takes each part of the engine with something to run out of it, with the actors in
    /// the part and what they react to, and shares the parts among the workers that are to run, by what their turns
    /// are expected to cost (ADR-0011). A step for one worker takes nothing out: it is run whole, where its parts
    /// lie, by [`Engine::run_whole`].
    pub fn plan(&mut self, workers: Workers) -> Plan {
        self.step += 1;

        // The parts in the order of their numbers, as a step runs them.
        self.waiting.sort_unstable();
        self.waiting.dedup();
        self.packing.parts.clear();
        for index in 0..self.waiting.len() {
            let part = self.waiting[index];
            if self.has_work(part) {
                self.packing.parts.push((part, self.expected[part]));
            }
        }
        self.waiting.clear();
        let work = self.packing.parts.iter().map(|(_, cost)| cost).sum();
        let (delta, running) = plan::plan(&self.packing.parts, self.recent, workers);
        self.recent = toward(self.recent, work);
        let running = running.max(1);

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer, each given an
        // equal share of the host's quota for mail. Each worker seats the children its turns spawn in every
        // `running`th part from its place, of all the parts and not only the ones it runs, so children spread across
        // the parts and no two workers give the same seat.
        let parity = parity(self.step);
        let share = self.mail_bytes / (2 * (ROOTS + 1 + workers.max.max(1)));
        for arena in &mut self.arenas[parity] {
            arena.clear(share);
        }
        if running == 1 {
            let (arena, lists) = self.writer(parity, 0, running, share);

            return Plan { delta, split: Split::Whole(Whole { arena, lists }) };
        }

        self.packing.pack(running);
        let mut shares = Vec::with_capacity(running);
        for place in 0..running {
            let (arena, lists) = self.writer(parity, place, running, share);
            let mut parcels = self.spare.parcels.pop().unwrap_or_default();
            for index in 0..self.packing.shares[place].1.len() {
                let part = self.packing.shares[place].1[index];
                parcels.push(self.parcel(part));
            }
            shares.push(Share { parcels, arena, lists });
        }

        Plan { delta, split: Split::Shares(shares) }
    }

    /// The arena and lists of the writer in place `place` of `running`, for steps of `parity`, its arena given
    /// `share` bytes: taken out to fill, with the seats it gives the children its turns spawn, in every `running`th
    /// part from its place.
    fn writer(&mut self, parity: usize, place: usize, running: usize, share: usize) -> (Arena, Lists) {
        let slot = ROOTS + 1 + place;
        let arena = self.arena(parity, slot, share);
        while self.lists.len() <= slot {
            let lists = self.lists();
            self.lists.push(lists);
        }
        // What is left in its place allocates nothing, and the lists come back to the same place, so each stays with
        // the worker that fills it.
        let mut lists = mem::take(&mut self.lists[slot]);
        if running == 1 {
            lists.seats = self.lend_seats();
        } else {
            self.seats(place, running, &mut lists.seats);
        }

        (arena, lists)
    }

    /// Takes in what a writer left once the parts it ran are back: how long its turns took, its arena, the seats it
    /// did not give out, and the children its turns stood up, seated; and keeps its lists in their place.
    fn take_in(&mut self, mut lists: Lists, arena: Arena) {
        self.costs.merge(&mut lists.tally);
        let slot = arena.slot();
        self.arenas[parity(self.step)][slot] = arena;
        self.take_back(&mut lists.seats);
        self.free.append(&mut lists.freed);
        let mut stood = mem::take(&mut lists.stood);
        while let Some(child) = stood.pop() {
            self.seat(child.seated(), child.path().clone(), child.above(), child.into_actor());
        }
        lists.stood = stood;
        self.lists[slot] = lists;
    }

    /// Puts each ending the watches handed on, `forward`, in the stream of the actor that watches, and keeps
    /// the list for its room.
    fn hand_on(&mut self, mut forward: Vec<Notice>) {
        while let Some(notice) = forward.pop() {
            let part = self.part(notice.to.seat);
            self.streams.shard_mut(notice.to.seat.number()).end(notice);
            self.expected[part] = self.expect(part);
            self.waiting.push(part);
        }
        self.spare.forward = forward;
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
        let running = done.len();
        // Every part goes back before any child is seated: a worker seats its children in parts another worker may
        // have run, and putting that part back would put its seats back over them.
        let mut left = mem::take(&mut self.spare.left);
        for Done { lists, arena, mut parcels } in done {
            while let Some(parcel) = parcels.pop() {
                self.put(parcel);
            }
            self.spare.parcels.push(parcels);
            left.push((lists, arena));
        }
        // Taken in the order the workers were given, so a step seats the same children in the same order every time.
        left.reverse();
        while let Some((lists, arena)) = left.pop() {
            self.take_in(lists, arena);
        }
        self.spare.left = left;

        self.packing.parts.clear();
        let mut touched = self.touched();
        for &part in &touched {
            let records: usize = self.lists.iter().map(|lists| lists.routes.records(part)).sum();
            self.packing.parts.push((part, u64::try_from(records).unwrap_or(u64::MAX)));
            self.waiting.push(part);
        }
        touched.clear();
        self.spare.touched = touched;
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

    /// Ends a step: puts back every part the workers folded, and puts the endings their watches handed on in the
    /// streams of the actors that watch.
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

        self.hand_on(forward);
    }

    /// The parts some writer routed a record to since the last fold, each once and in the order of their numbers: the
    /// parts a fold folds.
    fn touched(&mut self) -> Vec<usize> {
        let mut touched = mem::take(&mut self.spare.touched);
        for part in 0..self.seating.len() {
            let records: usize = self.lists.iter().map(|lists| lists.routes.records(part)).sum();
            if records > 0 {
                touched.push(part);
            }
        }

        touched
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
        let empty = Seating { seated: BySeat::empty(parts) };
        let rows = &mut self.lists;
        self.table.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.table), &mut pile.table);
        self.log.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.log), &mut pile.log);
        self.lives.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.lives), &mut pile.lives);
        self.streams.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.streams), &mut pile.streams);
        self.watches.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.watches), &mut pile.watches);
        pile.seating = mem::replace(&mut self.seating[part], empty);
        pile.part = part;
        pile.expected = 0;

        pile
    }

    /// Puts back a folded part from `pile`, leaving it empty but for what its watches handed on.
    fn unpile(&mut self, pile: &mut fold::Pile) {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let part = pile.part;
        let rows = &mut self.lists;
        self.table.empty(part, &mut pile.table, rows.iter_mut().map(|lists| &mut lists.routes.table));
        self.log.empty(part, &mut pile.log, rows.iter_mut().map(|lists| &mut lists.routes.log));
        self.lives.empty(part, &mut pile.lives, rows.iter_mut().map(|lists| &mut lists.routes.lives));
        self.streams.empty(part, &mut pile.streams, rows.iter_mut().map(|lists| &mut lists.routes.streams));
        self.watches.empty(part, &mut pile.watches, rows.iter_mut().map(|lists| &mut lists.routes.watches));
        let empty = Seating { seated: BySeat::empty(parts) };
        self.seating[part] = mem::replace(&mut pile.seating, empty);
        self.expected[part] = pile.expected;
    }

    /// Takes part `part` out of the engine for a worker to run.
    fn parcel(&mut self, part: usize) -> Parcel {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let empty = Seating { seated: BySeat::empty(parts) };

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
            self.seat(child.seated(), child.path().clone(), child.above(), child.into_actor());
        }
    }

    /// Empty lists for a worker, with a row of buckets for each view.
    fn lists(&self) -> Lists {
        let routes = Routes {
            table: self.table.routed(),
            log: self.log.routed(),
            lives: self.lives.routed(),
            streams: self.streams.routed(),
            watches: self.watches.routed(),
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
    fn seat(&mut self, me: Seated, path: ActorPath, above: Above, actor: Box<dyn Runs>) {
        let kind = self.kinds.number(path.last().kind());
        let part = self.part(me.seat);
        self.seating[part].seated.insert(me, Sitting { path, above, kind });
        self.actors[part].insert(me, Held::Here(actor));
    }

    /// The part `seat` falls in.
    fn part(&self, seat: SeatNumber) -> usize {
        part(seat, self.seating.len())
    }

    /// Gives `seats` the seats the writer in place `place` of `running` gives out: the new numbers of every
    /// `running`th part from its place, and for the first writer, every empty seat. An empty seat is given out once,
    /// by whichever writer holds it.
    fn seats(&mut self, place: usize, running: usize, seats: &mut Seats) {
        seats.clear(new_seat(self.next.len()));
        for part in (place..self.next.len()).step_by(running) {
            let free = if part == 0 {
                self.free.len()
            } else {
                0
            };
            seats.give(self.free.drain(..free), self.next[part]);
        }
    }

    /// Takes back the seats a writer did not give out, and where each part's new numbers have got to, leaving
    /// `seats` empty.
    fn take_back(&mut self, seats: &mut Seats) {
        let parts = self.next.len();
        let (free, next) = seats.drain();
        self.free.extend(free);
        for number in next {
            self.next[part(SeatNumber::new(number), parts)] = number;
        }
    }

    /// Every seat, the empty ones and each part's new numbers, lent to one writer whole: to the one worker of a step
    /// run whole, or to a root's `begin`. [`Self::take_back_lent`] takes them back.
    fn lend_seats(&mut self) -> Seats {
        let stride = new_seat(self.next.len());

        Seats::new(mem::take(&mut self.free), mem::take(&mut self.next), stride)
    }

    /// Takes back the seats lent whole by [`Self::lend_seats`].
    fn take_back_lent(&mut self, seats: Seats) {
        (self.free, self.next) = seats.into_parts();
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: in each of its parts, one turn for each item an actor there was handed, and
    /// each closing turn to run, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was handed behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let Share { mut parcels, arena, mut lists } = share;
        let mut tally = mem::take(&mut lists.tally);
        // One list of what turns ask for, over the share: each turn adds to it, and its effects are routed out of it
        // as it finishes.
        let mut asked = Asked::continuing(mem::take(&mut lists.effects), arena, mem::take(&mut lists.seats));
        for parcel in &mut parcels {
            self.run_part(parcel.room(), clock, &mut tally, &mut asked, &mut lists);
        }
        let (effects, _, arena, seats) = asked.into_parts();
        lists.effects = effects;
        lists.seats = seats;
        lists.tally = tally;

        Done { lists, arena, parcels }
    }

    /// Runs one part: hands out what has arrived, then runs every actor with items, in the order of the seats, and
    /// every actor whose closing turn is to run. Items for an actor that is not live are never handed to it: one
    /// never made, one gone, or one that has ended, which runs its closing turn instead.
    fn run_part(
        &self,
        mut room: Room<'_>,
        clock: &impl Clock,
        tally: &mut Tally,
        asked: &mut Asked,
        lists: &mut Lists,
    ) {
        let mut runs = mem::take(&mut lists.runs);
        runs.extend(room.streams.hand());
        for (seat, bits, range) in &runs {
            let me = Seated { id: ActorId::new(*bits), seat: SeatNumber::new(*seat) };
            let is_live = room.lives.life(me).is_some_and(lives::Life::is_live);
            if is_live {
                self.turns(&mut room, me, Work::Items(range.clone()), clock, tally, asked, lists);
            }
        }
        runs.clear();
        lists.runs = runs;

        for me in room.lives.take_ended() {
            self.turns(&mut room, me, Work::Closing, clock, tally, asked, lists);
        }
    }

    /// The bytes of a piece of mail sent in the last step.
    fn bytes(&self, payload: Payload) -> &[u8] {
        self.mail.get(payload.slot()).and_then(|arena| arena.read(payload)).unwrap_or_default()
    }

    /// The turns of the actor `me` names in `parcel` that `work` asks for, each measured into `tally`, what they
    /// ask for added to `asked` and routed into `lists`. The actor is taken out of its seat while its turns
    /// run, and put back, or its seat freed once its closing turn has run. Nothing when another actor sits there.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    fn turns(
        &self,
        room: &mut Room<'_>,
        me: Seated,
        work: Work,
        clock: &impl Clock,
        tally: &mut Tally,
        asked: &mut Asked,
        lists: &mut Lists,
    ) {
        let Room { streams, lives, seating, actors } = room;
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
                asked.next_turn();
                let begins = asked.effects_mut().len();
                let why = why(how);
                let mut ending = Ending::new(me, &seat.path, seat.above, self.table, asked, life.sent(), why);
                let ended = tally.time(key(ItemKind::Closing), clock, || actor.end(&mut ending));
                let effects = asked.effects_mut();
                // A closing turn that fails asks for nothing: only its log lines are kept, and the engine adds its
                // fault to them (ADR-0009). Either way the engine adds that the actor's life is over, after what the
                // turn sent.
                if let Err(fault) = ended {
                    keep_lines(effects, begins);
                    effects.push(Effect::new(me, What::EndFailed(fault)));
                }
                effects.push(Effect::new(me, What::Ended(how.clone())));

                let ran = Ran { effects: begins..effects.len(), stood: Vec::new(), context: *context };
                lists.route(ran, effects, me.id);
                // Whatever reaches an actor that has gone is dropped when the engine would hand it over.
                seating.seated.remove(me);
                lists.freed.push(me.seat);
                actors.remove(me);
            }
            (Work::Items(range), State::Live) => {
                let mut stood_since = ByActor::default();
                let sent_before = life.sent();
                let mut count = sent_before;
                for handed in streams.handed(range) {
                    let table = Since { table: self.table, stood: &stood_since };
                    asked.next_turn();
                    let begins = asked.effects_mut().len();
                    let source = handed.source();
                    let (item, context) = match handed {
                        Handed::Mail { id: cause, envelope: &Envelope { kind, bytes, root, depth, .. } } => {
                            let bytes = self.bytes(bytes);
                            let mail = Mail::new(kind, bytes, source, root, depth, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(cause), root, depth: depth + 1 })
                        }
                        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(me, &seat.path, seat.above, &table, asked, count, source);
                    let handled = tally.time(key(item_kind(handed)), clock, || actor.handle(item, &mut turn));
                    let ran = Ran::of(me, handled, asked, context, begins);

                    if !ran.stood.is_empty() {
                        let effects = &asked.effects_mut()[ran.effects.clone()];
                        for (child, sent) in senders(effects).into_iter().filter(|(actor, _)| actor.id != me.id) {
                            let (key, record) = lives::sent(child, sent);
                            lists.routes.lives.put(key, record);
                        }
                    }
                    let stood =
                        ran.stood.iter().map(|child| (child.id().bits(), (child.path().clone(), child.seated().seat)));
                    stood_since.extend(stood);
                    // Most turns of a busy actor ask for nothing, and then there is nothing to route.
                    let asked_nothing = ran.effects.is_empty() && ran.stood.is_empty();
                    let seen = if asked_nothing {
                        Seen::default()
                    } else {
                        lists.route(ran, asked.effects_mut(), me.id)
                    };
                    count = seen.sent.unwrap_or(count);
                    if seen.ends {
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

impl Parcel {
    /// The part, for its turns to change.
    fn room(&mut self) -> Room<'_> {
        Room {
            streams: &mut self.streams,
            lives: &mut self.lives,
            seating: &mut self.seating,
            actors: &mut self.actors,
        }
    }
}

impl Lists {
    /// Routes a finished turn's effects into this worker's row of buckets for each view, taking them out of the
    /// list, keeps the children the turn stood up, to be seated, and answers with what it saw of the actor `me`.
    ///
    /// Each effect is routed into every view in one pass: the other views borrow it, then the streams take it by
    /// value. A turn that stood up a child whose `begin` failed is routed without what that child left.
    fn route(&mut self, ran: Ran, list: &mut Vec<Effect>, me: ActorId) -> Seen {
        let Ran { effects, stood, context } = ran;
        let routes = &mut self.routes;
        let mut seen = Seen::default();
        match begun(&list[effects.clone()]) {
            Cow::Borrowed(_) => {
                for effect in list.drain(effects.start..) {
                    seen.see(&effect, me);
                    routes.route(&effect, &context);
                    routes.streams.route_one(effect, &context);
                }
            }
            Cow::Owned(applied) => {
                list.truncate(effects.start);
                for effect in applied {
                    seen.see(&effect, me);
                    routes.route(&effect, &context);
                    routes.streams.route_one(effect, &context);
                }
            }
        }

        self.stood.extend(stood);
        seen
    }
}

impl Routes {
    /// How many records this row holds for part `part`, across the views.
    fn records(&self, part: usize) -> usize {
        let views = self.table.records(part) + self.log.records(part) + self.lives.records(part);

        views + self.streams.records(part) + self.watches.records(part)
    }

    /// Routes one effect, emitted in `context`, into every view's bucket but the streams'.
    fn route(&mut self, effect: &Effect, context: &Context) {
        self.table.route(effect, context);
        self.log.route(effect, context);
        self.lives.route(effect, context);
        self.watches.route(effect, context);
    }
}

impl Ran {
    /// The turn of the actor `me` names that returned `outcome`, having asked for what is in `asked` from `begins` on,
    /// in `context`.
    ///
    /// A turn that fails changes nothing outside its actor: what it asked for is dropped but for its log lines,
    /// and the engine writes the one effect that says the actor ended (ADR-0009). The seats of the children it
    /// stood up are given back.
    fn of(me: Seated, outcome: Result<(), Fault>, asked: &mut Asked, context: Context, begins: usize) -> Self {
        let mut stood = asked.take_stood();
        if let Err(fault) = outcome {
            keep_lines(asked.effects_mut(), begins);
            asked.effects_mut().push(Effect::new(me, What::Failed(fault)));
            for child in mem::take(&mut stood) {
                asked.seats_mut().give_back(child.seated().seat);
            }
        }

        Self { effects: begins..asked.effects_mut().len(), stood, context }
    }
}

/// Puts the actor `me` names back in its seat in `actors` after its turns.
fn put_back(actors: &mut BySeat<Held>, me: Seated, actor: Box<dyn Runs>) {
    if let Some(held) = actors.get_mut(me) {
        *held = Held::Here(actor);
    }
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

/// What routing a turn's effects saw of the actor whose turn it was.
#[derive(Default)]
struct Seen {
    /// The count of the last mail it sent, when it sent any.
    sent: Option<u64>,
    /// Whether it ended.
    ends: bool,
}

impl Seen {
    /// Notes `effect`, when it is by the actor `me`.
    fn see(&mut self, effect: &Effect, me: ActorId) {
        let is_own = effect.by() == me;
        if !is_own {
            return;
        }
        if let What::Send { count, .. } = effect.what() {
            self.sent = Some(self.sent.map_or(*count, |sent| sent.max(*count)));
        }
        self.ends |= ending(effect, me).is_some();
    }
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
