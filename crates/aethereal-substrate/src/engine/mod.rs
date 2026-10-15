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

/// What one worker's share of a step left: its turns, the parts it ran, and how long the turns took.
pub struct Done {
    lists: Lists,
    tally: Tally,
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
    /// Boxed, as the lists pass between threads each step.
    routes: Box<Routes>,
    stood: Vec<Stood>,
    seats: Seats,
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
    /// most `read_limit` bytes into any one piece of mail.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize) -> Self {
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
        // children its `begin` stands up take the empty seats first, then new ones, in any part.
        let parity = parity(self.step);
        let mut seats = self.seats(0..self.seating.len());
        let me = Seated { id, seat: seats.take() };
        let mut asked = Asked::writing(self.arena(parity, ROOTS), seats);
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
                self.take_back(seats);

                return Err(Refused::Began(fault));
            }
        };
        self.take_back(seats);
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

        let parts: Vec<(usize, u64)> =
            (0..self.seating.len()).filter_map(|part| Some((part, self.expect(part)?))).collect();
        let work = parts.iter().map(|(_, cost)| cost).sum();
        let (delta, packed) = plan::plan(&parts, self.recent, workers);
        self.recent = toward(self.recent, work);

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer. Each worker
        // seats the children its turns spawn in every `running`th part from its place, of all the parts and not only
        // the ones it runs, so children spread across the parts and no two workers give the same seat.
        let parity = parity(self.step);
        for arena in &mut self.arenas[parity] {
            arena.clear();
        }
        let running = packed.len();
        let mut shares = Vec::with_capacity(running);
        for ((parts, slot), place) in packed.into_iter().zip(ROOTS + 1..).zip(0..) {
            let arena = self.arena(parity, slot);
            while self.lists.len() <= slot {
                let lists = self.lists();
                self.lists.push(lists);
            }
            let mut lists = mem::take(&mut self.lists[slot]);
            lists.seats = self.seats((place..self.seating.len()).step_by(running.max(1)));
            let parcels = parts.into_iter().map(|part| self.parcel(part)).collect();
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

    /// Ends a step: puts back the parts the workers ran, takes in how long their turns took, and applies what the
    /// turns asked for (ADR-0011).
    ///
    /// Each view folds a turn by itself, reading no other, and comes out the same in whatever order the turns come,
    /// so each worker's turns are folded as they are. Once every turn is folded the streams settle, pairing the
    /// step's watches and endings.
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply(&mut self, done: Vec<Done>) {
        let parity = parity(self.step);
        let mut rows = Vec::with_capacity(done.len());
        for Done { mut lists, tally, arena, parcels } in done {
            self.costs.merge(tally);
            let slot = arena.slot();
            self.arenas[parity][slot] = arena;
            for parcel in parcels {
                self.put(parcel);
            }
            let seats = mem::take(&mut lists.seats);
            self.take_back(seats);
            for child in lists.stood.drain(..) {
                self.seat(child.seated(), child.path().clone(), child.into_actor());
            }
            rows.push((slot, lists));
        }

        // Each part of each view takes its column: the bucket every worker filled for it.
        self.table.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.table));
        self.log.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.log));
        self.lives.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.lives));
        self.streams.fold_routed(rows.iter_mut().map(|(_, lists)| &mut lists.routes.streams));
        self.streams.settle();
        for (slot, lists) in rows {
            self.lists[slot] = lists;
        }
    }

    /// What the turns waiting in part `part` are expected to cost, in nanoseconds: one for each item that has
    /// arrived for an actor seated there, and each closing turn to run. None when the part has nothing to run.
    fn expect(&self, part: usize) -> Option<u64> {
        let streams = self.streams.at(part);
        let lives = self.lives.at(part);
        let has_work = streams.has_items() || lives.has_ended();
        if !has_work {
            return None;
        }

        let seated = &self.seating[part].seated;
        let cost = |me: Seated, item: ItemKind| {
            let kind = seated.get(me).map(|sitting| sitting.kind);

            kind.map_or(0, |actor| self.costs.expect(&CostKey { actor, item }))
        };
        let items: u64 = streams.arrived().map(|(me, handed)| cost(me, item_kind(handed))).sum();
        let closings: u64 = lives.ended().iter().map(|me| cost(*me, ItemKind::Closing)).sum();

        Some(items + closings)
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
        let part = self.part(me.seat);
        self.seating[part].seated.insert(me, Sitting { path, kind });
        self.actors[part].insert(me, Held::Here(actor));
    }

    /// The part `seat` falls in.
    fn part(&self, seat: SeatNumber) -> usize {
        part(seat, self.seating.len())
    }

    /// The seats a writer gives out in the parts `parts`: their free seats, then their new numbers.
    fn seats(&mut self, parts: impl Iterator<Item = usize>) -> Seats {
        let mut free = Vec::new();
        let mut next = Vec::new();
        for part in parts {
            let seating = &mut self.seating[part];
            free.append(&mut seating.free);
            next.push(seating.next);
        }

        Seats::new(free, next, new_seat(self.seating.len()))
    }

    /// Takes back the seats a writer did not give out, into the parts they fall in, and where each part's new
    /// numbers have got to.
    fn take_back(&mut self, seats: Seats) {
        let (free, next) = seats.into_parts();
        for seat in free {
            let part = self.part(seat);
            self.seating[part].free.push(seat);
        }
        for number in next {
            let part = self.part(SeatNumber::new(number));
            self.seating[part].next = number;
        }
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: in each of its parts, one turn for each item an actor there was handed, and
    /// each closing turn to run, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was handed behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let mut tally = Tally::default();
        let Share { mut parcels, mut arena, mut lists } = share;
        for parcel in &mut parcels {
            self.run_part(parcel, clock, &mut tally, &mut arena, &mut lists);
        }

        Done { lists, tally, arena, parcels }
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
                let start = clock.nanos();
                let ended = actor.end(&mut Ending::new(me, &seat.path, self.table, &mut asked, life.sent(), why));
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
                // Whatever reaches an actor that has gone is dropped when the engine would hand it over.
                seating.seated.remove(me);
                seating.free.push(me.seat);
                actors.remove(me);
            }
            (Work::Items(range), State::Live) => {
                let mut stood_since = ByActor::default();
                let mut count = life.sent();
                for handed in streams.handed(range) {
                    let table = Since { table: self.table, stood: &stood_since };
                    let begins = lists.effects.len();
                    let mut asked =
                        Asked::continuing(mem::take(&mut lists.effects), mem::take(arena), mem::take(&mut lists.seats));
                    let source = handed.source();
                    let (item, context) = match handed {
                        Handed::Mail(&Envelope { id: cause, kind, bytes, root, depth, .. }) => {
                            let bytes = self.bytes(bytes);
                            let mail = Mail::new(kind, bytes, source, root, depth, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(cause), root, depth: depth + 1 })
                        }
                        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(me, &seat.path, &table, &mut asked, count, source);
                    let start = clock.nanos();
                    let handled = actor.handle(item, &mut turn);
                    tally.add(key(item_kind(handed)), clock.nanos().saturating_sub(start));
                    let ran = Ran::of(me, handled, asked, context, begins, lists, arena);

                    let effects = &lists.effects[ran.effects.clone()];
                    count = sent(effects, me.id).unwrap_or(count);
                    let stood =
                        ran.stood.iter().map(|child| (child.id().bits(), (child.path().clone(), child.seated().seat)));
                    stood_since.extend(stood);
                    let over = ends(effects, me.id);
                    lists.route(ran);
                    if over {
                        break;
                    }
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
const fn item_kind(handed: Handed<'_>) -> ItemKind {
    match handed {
        Handed::Mail(envelope) => ItemKind::Mail(envelope.kind),
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
