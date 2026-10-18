//! The engine: it runs turns in steps, and applies what they asked for (ADR-0009, ADR-0011).

mod batch;
mod birth;
mod costs;
mod fold;
mod fuse;
mod made;
mod plan;
mod rehearsal;
mod since;
#[cfg(test)]
mod tests;

pub use costs::Tally;
pub use fold::{Fold, Folding};
pub use fuse::Hops;
pub use plan::{Plan, Share, Split, SplitCost, Whole, Workers};

use crate::id::root_id;
use crate::log;
use crate::stream::{self, Handed, Streams};
use crate::table::Actors;
use crate::watch::{Notice, Watches};
use crate::{
    ActorTable, Blobs, ByActor, BySeat, Clock, Context, Entry, Envelope, Folded, Lives, Log, Part, Routed, State,
    lives, rounded,
};
use aethereal_actor::pins::{self, Change};
use aethereal_actor::{
    Above, Actor, Arena, Asked, Beginning, Birth, BlobHash, BlobRef, Buffers, Chain, Closing, Code, Declared, Effect,
    Fault, Guests, How, Instances, Item, Links, Mail, MailId, Mailed, NoGuests, Preparing, Puts, Reads, Refused, Runs,
    SeatNumber, Seated, Seats, Stood, Turning, Unfit, What, Why, config_bytes,
};
use aethereal_data::{ActorId, ActorPath, KindId, PathSegment};
use birth::Births;
use costs::{CostKey, Costs, ItemKind, toward};
use fuse::{Fusing, Hand, Hop, Stepped};
use made::Made;
use plan::Packing;
use since::{Sees, Since};
use std::mem;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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
    /// The instances each part's actors run their code in, which the engine carries with the part and never looks
    /// into (ADR-0015).
    instances: Vec<Instances>,
    /// The guests' code the engine runs, which each part's instances carry (ADR-0015): none for an engine made
    /// without it.
    guests: Arc<dyn Guests>,
    /// The native types the program keeps by name, which a guest may spawn, and each part's instances carry
    /// (ADR-0009): none for an engine made without them.
    links: Arc<Links>,
    /// The seats each part gives the children its turns spawn: its own free seats and new numbers, and those set
    /// aside for it in other parts (ADR-0013).
    seats: Vec<Seats>,
    /// How many actors sit in each part.
    seated: Vec<usize>,
    /// How many actors sit in all the parts.
    actors_seated: usize,
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
    /// The writers the last plan handed lists to: the only rows of `lists` that step's turns route records into.
    writing: Range<usize>,
    /// How long turns take, as measured so far.
    costs: Costs,
    /// The expected work of recent steps, in nanoseconds: a moving average weighted to the most recent.
    recent: u64,
    /// The number of the last step run. No step has run at zero.
    step: u64,
    /// The most bytes one read of a piece of mail may allocate.
    read_limit: usize,
    /// The most fused hops one part runs in one step (ADR-0013).
    hops: usize,
    /// The most nanoseconds one actor's turns take in a step before the rest are carried over: none on one worker.
    slice: u64,
    /// How the sends made while a part could still fuse have fared, over every step so far.
    hopped: Hops,
    /// The host's quota for the bytes of mail in flight, shared equally among the arenas of every writer.
    mail_bytes: usize,
    /// The blobs the engine holds, with their pins (ADR-0014).
    blobs: Blobs,
    /// Where each writer holds the blobs its turns put, by its slot: kept from step to step, so each blob it puts
    /// has an id no other has had.
    puts: Vec<Puts>,
    /// The changes to pins made where no worker's buffer was open, taken at each sweep: kept for its room.
    unbuffered: Vec<Change>,
    /// Which engine of the process this is, in the id of each blob it holds. Last, so what the engine's actors held
    /// is let go of before the engine is forgotten.
    number: Number,
}

/// Why code put between steps was refused (ADR-0015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeRefused {
    /// Its bytes would take the blobs past the host's budget.
    NoRoom,
    /// Its guests cannot run it.
    Unfit(Unfit),
}

/// Which engine of the process an engine is: the engines a process makes are numbered in turn.
struct Number(u32);

/// The number the next engine the process makes takes.
static ENGINES: AtomicU32 = AtomicU32::new(0);

impl Number {
    /// The next engine's number.
    ///
    /// # Panics
    ///
    /// Panics once a process has made `u32::MAX` engines.
    fn next() -> Self {
        let number = ENGINES.fetch_add(1, Ordering::Relaxed);
        assert!(number < u32::MAX, "no process makes u32::MAX engines");

        Self(number)
    }
}

impl Drop for Number {
    /// Forgets what was left for the engine's pins once its actors and blobs have gone.
    fn drop(&mut self) {
        pins::forget(self.0);
    }
}

/// One part of the engine's seats: the actors in them.
struct Seating {
    /// Each actor in a seat of this part.
    seated: BySeat<Sitting>,
}

/// An actor in its seat: what is fixed when it is seated, and the children it has made since.
struct Sitting {
    /// Where the actor stands.
    path: ActorPath,
    /// What stands above it.
    above: Above,
    /// Its kind: its type's id.
    kind: KindId,
    /// The children it has made, noted as each is seated: its own knowledge of the keys it has used (ADR-0008).
    made: Made,
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
    /// Which engine of the process it reads, whose pins its turns change.
    engine: u32,
    table: &'a ActorTable,
    /// The blobs the store held as the step began.
    blobs: &'a Blobs,
    /// The arenas the last step's mail was written to, by writer.
    mail: &'a [Arena],
    read_limit: usize,
    /// The most fused hops one part runs in the step.
    hops: usize,
    /// The most nanoseconds one actor's turns take in the step before the rest of its items are carried over.
    slice: u64,
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
    left: Vec<(Lists, Arena, Puts)>,
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
    instances: Instances,
    seats: Seats,
}

/// One part of the engine as its turns change it, where it lies: in a parcel a worker owns, or in the engine.
struct Room<'a> {
    streams: &'a mut stream::Shard,
    lives: &'a mut lives::Shard,
    seating: &'a mut Seating,
    actors: &'a mut BySeat<Held>,
    instances: &'a mut Instances,
    seats: &'a mut Seats,
}

/// What one worker's share of a step left: its turns and how long they took, and the parts it ran.
pub struct Done {
    lists: Lists,
    /// The arena the worker wrote its mail to, to keep for the step after next.
    arena: Arena,
    /// The blobs the worker's turns put, for the store to take in.
    puts: Puts,
    parcels: Vec<Parcel>,
}

/// What a worker fills in a step: its row of buckets for each view and for the children its turns stood up, each
/// actor it ran, and the seats it gives the children. The engine empties it and hands it back the next step, so nothing in
/// it is allocated again.
#[derive(Default)]
pub struct Lists {
    /// What the turn running asks for, routed into the buckets when it finishes.
    buffers: Buffers,
    routes: Routes,
    /// The seats of the actors whose closing turns ran, empty once the step is applied: no seat is given again in the
    /// step that freed it, while its actor's last records are still to fold.
    freed: Vec<SeatNumber>,
    /// How long the turns took, by what they were.
    tally: Tally,
    /// Each actor with items in the part running, as it was handed them: kept for its room.
    runs: Vec<stream::Run>,
    /// What the part running keeps of its step while it fuses chains of mail.
    fusing: Fusing,
    /// The changes to blobs' pins its turns made, noted on the worker's thread as they ran (ADR-0014).
    pins: Vec<Change>,
}

/// One writer's row of buckets for each view, and for the children its turns stood up, to seat.
#[derive(Default)]
struct Routes {
    table: Routed<Actors>,
    log: Routed<Log>,
    lives: Routed<Lives>,
    streams: Routed<Streams>,
    watches: Routed<Watches>,
    births: Births,
}

/// What an actor runs in a step.
enum Work {
    /// One turn for each item its part of the streams handed out for this run, in order.
    Items(stream::Run),
    /// Its closing turn.
    Closing,
}

/// One finished turn, as its worker routes it.
struct Ran {
    /// The children it stood up.
    stood: Vec<Stood>,
    /// The context its effects were emitted in.
    context: Context,
}

impl Engine {
    /// An engine with no actors, its views in `parts` parts [`rounded`] up to a power of two, whose turns read at
    /// most `read_limit` bytes into any one piece of mail, whose mail in flight is written to `mail_bytes` of
    /// arenas, shared equally among the writers, two arenas each, whose blobs hold at most `blob_bytes` (ADR-0014),
    /// and whose parts each run at most `hops` fused hops in a step (ADR-0013). With no hops, every piece of mail is
    /// handled in a later step.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize, mail_bytes: usize, blob_bytes: usize, hops: usize) -> Self {
        let parts = rounded(parts);
        let number = Number::next();

        Self {
            table: ActorTable::new(parts),
            streams: Folded::new(parts),
            watches: Folded::new(parts),
            log: Folded::new(parts),
            lives: Folded::new(parts),
            seating: (0..parts.get()).map(|_| Seating { seated: BySeat::empty(parts) }).collect(),
            actors: (0..parts.get()).map(|_| BySeat::empty(parts)).collect(),
            waiting: Vec::new(),
            instances: (0..parts.get()).map(|_| Instances::default()).collect(),
            guests: Arc::new(NoGuests),
            links: Arc::default(),
            seats: (0..parts.get()).map(|part| Seats::of_part(new_seat(part), new_seat(parts.get()))).collect(),
            seated: vec![0; parts.get()],
            actors_seated: 0,
            expected: vec![0; parts.get()],
            packing: Packing::new(parts.get()),
            spare: Spare::default(),
            arenas: [Vec::new(), Vec::new()],
            lists: Vec::new(),
            writing: 0..0,
            costs: Costs::new(),
            recent: 0,
            step: 0,
            read_limit,
            hops,
            slice: u64::MAX,
            hopped: Hops::default(),
            mail_bytes,
            blobs: Blobs::new(number.0, blob_bytes),
            puts: Vec::new(),
            unbuffered: Vec::new(),
            number,
        }
    }

    /// The engine, running the guests' code `guests` holds (ADR-0015): set as the engine is made, before any actor.
    #[must_use]
    pub fn with_guests(mut self, guests: Arc<dyn Guests>) -> Self {
        self.guests = guests;
        self.carry();

        self
    }

    /// The engine, keeping the native types `links` by name, for a guest to spawn (ADR-0009): set as the engine is
    /// made, before any actor.
    #[must_use]
    pub fn with_links(mut self, links: Links) -> Self {
        self.links = Arc::new(links);
        self.carry();

        self
    }

    /// Gives each part instances that carry the engine's guests' code, the native types it keeps, and its read limit.
    fn carry(&mut self) {
        for instances in &mut self.instances {
            *instances = Instances::new(Arc::clone(&self.guests), Arc::clone(&self.links), self.read_limit);
        }
    }

    /// The table of actors, as of the last step.
    #[must_use]
    pub const fn table(&self) -> &ActorTable {
        &self.table
    }

    /// What `actor` logged most recently, oldest first, as of the last step.
    ///
    /// It holds the fault of a turn that failed, and of a closing turn that did, so it is read after the actor has
    /// ended, by the seat it sat in: the table no longer holds it (ADR-0008). Nothing for an actor that has logged
    /// nothing, or whose seat another actor has taken and logged in since.
    pub fn log(&self, actor: Seated) -> impl Iterator<Item = &Entry> {
        self.log.shard(actor.seat.number()).get(actor).into_iter().flatten()
    }

    /// The number of the last step run, from one. Zero before the first.
    #[must_use]
    pub const fn step_number(&self) -> u64 {
        self.step
    }

    /// How the sends made while a part could still fuse have fared, over every step so far: how many were fused, and
    /// why each other went out as mail (ADR-0013).
    #[must_use]
    pub const fn hopped(&self) -> Hops {
        self.hopped
    }

    /// Whether a step would run no turn: no actor has anything to react to, and none has a closing turn to run.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.waiting.iter().any(|part| self.has_work(*part))
    }

    /// Puts `bytes` as a blob, for the chassis, between steps, and answers with the first reference to it
    /// (ADR-0014): refused when they would take the blobs past the host's budget.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        let room = self.blobs.room();
        let mut puts = self.puts(ROOTS, room);
        let put = puts.pinned(bytes);
        self.blobs.take_in(&mut puts);
        self.puts[ROOTS] = puts;

        put
    }

    /// Puts `bytes` as a blob that is code, for the chassis, between steps, and answers with the first reference to it
    /// (ADR-0014, ADR-0015): refused when they would take the blobs past the host's budget, or are not code its guests
    /// can run, and then the blob goes at the next step's fold.
    ///
    /// # Errors
    ///
    /// Why the code was refused.
    pub fn put_code(&mut self, bytes: Vec<u8>) -> Result<BlobRef, CodeRefused> {
        let unchecked = bytes.clone();
        let code = self.put_blob(bytes).map_err(|_| CodeRefused::NoRoom)?;
        self.guests.load(code.id(), &unchecked).map_err(CodeRefused::Unfit)?;

        Ok(code)
    }

    /// The bytes of the blob `blob` refers to, as the store holds them between steps.
    #[must_use]
    pub fn blob(&self, blob: &BlobRef) -> Option<&[u8]> {
        self.blobs.read(blob.id())
    }

    /// The content hash of the blob `blob` refers to, as the store holds it between steps: the one kept, or one
    /// computed now and kept (ADR-0014). Nothing when this engine does not hold it.
    pub fn hash(&mut self, blob: &BlobRef) -> Option<BlobHash> {
        self.blobs.hash_now(blob.id())
    }

    /// How many blobs the engine holds, and their bytes.
    #[must_use]
    pub fn blobs_held(&self) -> (usize, usize) {
        self.blobs.held()
    }

    /// Makes a root of type `A` under `key`, for the chassis, and answers with its id (ADR-0008).
    ///
    /// The root's `begin` runs here, and what it asked for is applied before this returns, as a step's effects
    /// are. A root is made between steps.
    ///
    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds for another path (ADR-0008).
    pub fn add_root<A: Actor>(&mut self, key: &A::Key, config: A::Config) -> Result<ActorId, Refused> {
        let segment = PathSegment::new(A::NAME, key);

        let declared = Declared::of::<A>();
        self.add_root_made(&segment, Code::Linked(declared), |me, path, table, asked, instances| {
            let root = A::begin(config, &mut Beginning::new::<A>(me, path, table, asked, instances))
                .map_err(Refused::Began)?;

            Ok(Box::new(root))
        })
    }

    /// Makes a root of type `C` under `key` whose code is the blob `code` pins, for the chassis, and answers with its
    /// id (ADR-0009, ADR-0015): [`Self::add_root`] for a root a guest runs. Its `begin` runs in the instance of the
    /// blob of the part it is seated in.
    ///
    /// # Errors
    ///
    /// Refused when the key is taken, the blob makes no actor of `C`'s type, or `begin` fails.
    ///
    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds for another path (ADR-0008).
    pub fn add_root_from<C: Actor>(
        &mut self,
        code: &BlobRef,
        key: &C::Key,
        config: &C::Config,
    ) -> Result<ActorId, Refused> {
        let segment = PathSegment::new(C::NAME, key);
        let made = self.guests.made(code.id(), Declared::of::<C>()).ok_or(Refused::NotMade)?;
        let config = config_bytes(config).map_err(|_| Refused::NotMade)?;
        let guests = Arc::clone(&self.guests);

        self.add_root_made(&segment, Code::Blob(made), |me, path, table, asked, instances| {
            let birth = Birth { me, path, above: Above::Chassis, table, asked, instances, refers: &config.held };

            guests.begin(made, &config.bytes, birth)
        })
    }

    /// Makes a root at `segment`, from `from`, which `begin` makes as it is seated as `me`, at its path, reading the
    /// table, adding to what is asked, and lent its part's instances.
    fn add_root_made(
        &mut self,
        segment: &PathSegment,
        from: Code,
        begin: impl FnOnce(
            Seated,
            &ActorPath,
            &dyn aethereal_actor::Table,
            &mut Asked,
            &mut Instances,
        ) -> Result<Box<dyn Runs>, Refused>,
    ) -> Result<ActorId, Refused> {
        let id = root_id(segment);
        let path = ActorPath::from_root(segment);
        let is_taken = self.table.holds_root(id);
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        // Mail sent between steps is read in the next step, with the mail the last step's turns sent. The root sits in
        // the part with fewest actors, and the children its `begin` stands up beside it while the part has room, then
        // in whichever part has numbered fewest new seats.
        let parity = parity(self.step);
        let home = self.emptiest();
        let mut seats = self.lend_seats(home);
        let me = Seated { id, seat: seats.own() };
        let blob_room = self.blobs.room();
        let mut asked = Asked::writing(self.arena(parity, ROOTS, 0), seats, self.puts(ROOTS, blob_room));
        let stood_before = ByActor::default();
        let table = Since { table: &self.table, stood: &stood_before, sees: Sees::Step, blobs: &self.blobs };
        // The root begins in the part it is seated in, lent that part's instances.
        let made = begin(me, &path, &table, &mut asked, &mut self.instances[home]);
        let (mut buffers, stood, arena, mut seats, mut puts) = asked.into_parts();
        self.arenas[parity][ROOTS] = arena;
        // A root is made between steps, when nothing reads the store, so what its `begin` put is taken in at once.
        self.blobs.take_in(&mut puts);
        self.puts[ROOTS] = puts;
        let context = Context { root: MailId { sender: id, count: 0 }, depth: 1 };
        let root = match made {
            Ok(root) => root,
            Err(refused) => {
                seats.give_back(me.seat);
                for child in stood {
                    seats.give_back(child.seated().seat);
                }
                self.take_back_lent(home, seats);
                // A `begin` that fails asks for nothing, as a failed turn does, but for the endings its host reported
                // of other actors, which are folded as a root's effects are; its lines are dropped (ADR-0009).
                keep_lines(&mut buffers, me);
                buffers.lines.clear();
                self.fold_began(buffers, Vec::new(), context, parity);

                return Err(refused);
            }
        };
        self.take_back_lent(home, seats);
        let made = segment.key().map_or(What::Root { from }, |key| What::RootInstanced { from, key: key.clone() });
        buffers.effects.insert(0, Effect::new(me, made));

        self.table.made_root(id);
        self.seat(me, path, Above::Chassis, root);
        self.fold_began(buffers, stood, context, parity);

        Ok(id)
    }

    /// Folds what a root's `begin` asked for, in `context`, between steps: its lines, its effects with the children
    /// it stood up, and its mail, into every view, and marks every part waiting for what they hand on.
    fn fold_began(&mut self, buffers: Buffers, stood: Vec<Stood>, context: Context, parity: usize) {
        let Buffers { mailed, lines, effects } = buffers;
        let written = &self.arenas[parity];
        self.log.fold_records(lines.into_iter().map(log::line), written);
        self.fold_turn(&effects, stood, context);
        let written = &self.arenas[parity];
        let sent = senders(&mailed).into_iter().map(|(actor, count)| lives::sent(actor, count));
        self.lives.fold_records(sent, written);
        self.watches.fold(&effects, &context, written);
        self.streams.fold_records(mailed.into_iter().map(|mailed| stream::entry(mailed, &context)), written);
        let mut forward = mem::take(&mut self.spare.forward);
        for shard in self.watches.shards_mut() {
            shard.settle(&mut forward);
        }
        self.hand_on(forward);
        for part in 0..self.seating.len() {
            self.expected[part] = self.expect(part);
        }
        self.waiting.extend(0..self.seating.len());
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
        let Whole { arena, puts, mut lists } = whole;
        let mut tally = mem::take(&mut lists.tally);
        let mut asked = Asked::continuing(mem::take(&mut lists.buffers), arena, Seats::default(), puts);
        pins::open(self.number.0, mem::take(&mut lists.pins));
        let mail = &self.arenas[parity(self.step + 1)];
        let reading = Reading {
            engine: self.number.0,
            table: &self.table,
            blobs: &self.blobs,
            mail,
            read_limit: self.read_limit,
            hops: self.hops,
            slice: self.slice,
        };
        for (part, _) in &self.packing.parts {
            let room = Room {
                streams: self.streams.at_mut(*part),
                lives: self.lives.at_mut(*part),
                seating: &mut self.seating[*part],
                actors: &mut self.actors[*part],
                instances: &mut self.instances[*part],
                seats: &mut self.seats[*part],
            };
            reading.run_part(room, clock, &mut tally, &mut asked, &mut lists);
        }
        let (buffers, _, arena, _, puts) = asked.into_parts();
        lists.buffers = buffers;
        lists.tally = tally;
        lists.pins = pins::close();

        Whole { arena, puts, lists }
    }

    /// Ends a step run by [`Engine::run_whole`] on the thread that calls it: takes in what its turns left, and folds
    /// each part some record reached where it lies, then puts the endings its watches hand on in the streams of the
    /// actors that watch.
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn apply_whole(&mut self, whole: Whole) {
        let Whole { arena, puts, lists } = whole;
        self.take_in(lists, arena, puts);
        self.sweep_blobs();

        let mut forward = mem::take(&mut self.spare.forward);
        let mut touched = self.touched();
        for &part in &touched {
            let written = &self.arenas[parity(self.step)];
            let rows = &mut self.lists[self.writing.clone()];
            for row in rows.iter_mut() {
                let bucket = row.routes.births.bucket_mut(part);
                let born = birth::seat_all(&mut self.seating[part], &mut self.actors[part], bucket);
                self.seated[part] += born;
                self.actors_seated += born;
            }
            self.table.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.table));
            self.log.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.log), written);
            self.lives.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.lives), written);
            self.streams.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.streams), written);
            self.watches.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.watches), written);
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
        self.rest();
        // An actor kept to a slice runs long enough for what splitting the step costs to be a small part of it, and
        // leaves what it carries for another worker, so on one worker none is kept to one.
        let slice = workers.split.among(workers.max).saturating_mul(SLICE_SPLITS).max(SLICE_LEAST);
        let shares_work = workers.max > 1;
        self.slice = if shares_work {
            slice
        } else {
            u64::MAX
        };

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
        for index in 0..self.packing.parts.len() {
            self.give_room(self.packing.parts[index].0);
        }
        let work = self.packing.parts.iter().map(|(_, cost)| cost).sum();
        let (delta, running, expected_nanos) = plan::plan(&self.packing.parts, self.recent, workers);
        self.recent = toward(self.recent, work);
        let running = running.max(1);
        self.writing = ROOTS + 1..ROOTS + 1 + running;

        // This step writes the arenas of its parity, which hold mail read two steps ago and no longer, each given an
        // equal share of the host's quota for mail. The seats children take are each part's own, given it above.
        let parity = parity(self.step);
        let share = self.mail_bytes / (2 * (ROOTS + 1 + workers.max.max(1)));
        for arena in &mut self.arenas[parity] {
            arena.clear(share);
        }
        // Each writer may put an equal share of what the budget for blobs has left (ADR-0014).
        let blob_share = self.blobs.room() / running;
        if running == 1 {
            let (arena, lists, puts) = self.writer(parity, 0, share, blob_share);

            return Plan { delta, split: Split::Whole(Whole { arena, puts, lists }), expected_nanos };
        }

        self.packing.pack(running);
        let mut shares = Vec::with_capacity(running);
        for place in 0..running {
            let (arena, lists, puts) = self.writer(parity, place, share, blob_share);
            let mut parcels = self.spare.parcels.pop().unwrap_or_default();
            for index in 0..self.packing.shares[place].1.len() {
                let part = self.packing.shares[place].1[index];
                parcels.push(self.parcel(part));
            }
            shares.push(Share { parcels, arena, puts, lists });
        }

        Plan { delta, split: Split::Shares(shares), expected_nanos }
    }

    /// The arena, lists, and puts of the writer in place `place`, for steps of `parity`, its arena given `share`
    /// bytes and its puts `blob_share`: taken out to fill.
    fn writer(&mut self, parity: usize, place: usize, share: usize, blob_share: usize) -> (Arena, Lists, Puts) {
        let slot = ROOTS + 1 + place;
        let arena = self.arena(parity, slot, share);
        let puts = self.puts(slot, blob_share);
        while self.lists.len() <= slot {
            let lists = self.lists();
            self.lists.push(lists);
        }
        // What is left in its place allocates nothing, and the lists come back to the same place, so each stays with
        // the worker that fills it.
        let lists = mem::take(&mut self.lists[slot]);

        (arena, lists, puts)
    }

    /// Takes in what a writer left once the parts it ran are back: how long its turns took, its arena, and the seats
    /// it did not give out; and keeps its lists in their place. The children its turns stood up are seated when their
    /// parts are folded.
    fn take_in(&mut self, mut lists: Lists, arena: Arena, mut puts: Puts) {
        self.costs.merge(&mut lists.tally);
        self.blobs.take_in(&mut puts);
        self.blobs.change(&mut lists.pins);
        self.puts[arena.slot()] = puts;
        self.hopped.add(mem::take(&mut lists.fusing.hops));
        let slot = arena.slot();
        self.arenas[parity(self.step)][slot] = arena;
        for seat in lists.freed.drain(..) {
            let part = self.part(seat);
            self.seated[part] -= 1;
            self.actors_seated -= 1;
            self.seats[part].free(seat);
        }
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

        Reading {
            engine: self.number.0,
            table: &self.table,
            blobs: &self.blobs,
            mail,
            read_limit: self.read_limit,
            hops: self.hops,
            slice: self.slice,
        }
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

    /// Begins the end of a step: puts back the parts the workers ran, takes in how long their turns took, and takes out
    /// each part some worker routed a record to, with every worker's records for it and the children stood up in it,
    /// one fold for each worker that ran the step, each part in the fold of the worker that last ran it. A part no
    /// record reached is not touched.
    pub fn gather(&mut self, done: impl ExactSizeIterator<Item = Done>) -> Vec<Fold> {
        let running = done.len();
        let mut left = mem::take(&mut self.spare.left);
        for Done { lists, arena, puts, mut parcels } in done {
            while let Some(parcel) = parcels.pop() {
                self.put(parcel);
            }
            self.spare.parcels.push(parcels);
            left.push((lists, arena, puts));
        }
        // Taken in the order the workers were given, so a step's costs are merged in the same order every time.
        left.reverse();
        while let Some((lists, arena, puts)) = left.pop() {
            self.take_in(lists, arena, puts);
        }
        self.spare.left = left;
        self.sweep_blobs();

        self.packing.parts.clear();
        let mut touched = self.touched();
        for &part in &touched {
            let records: usize = self.lists[self.writing.clone()].iter().map(|lists| lists.routes.records(part)).sum();
            self.packing.parts.push((part, u64::try_from(records).unwrap_or(u64::MAX)));
            self.waiting.push(part);
        }
        touched.clear();
        self.spare.touched = touched;
        if self.packing.parts.is_empty() {
            return Vec::new();
        }
        self.packing.pack_at_home(running);

        let mut folds = Vec::with_capacity(running);
        for index in 0..running {
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
        Folding { costs: &self.costs, slice: self.slice, written: &self.arenas[parity(self.step)] }
    }

    /// Ends a step: puts back every part the workers folded, and puts the endings their watches handed on in the
    /// streams of the actors that watch.
    pub fn settle(&mut self, folds: impl IntoIterator<Item = Fold>) {
        let mut forward = mem::take(&mut self.spare.forward);
        for Fold { parts: mut piles } in folds {
            while let Some(mut pile) = piles.pop() {
                forward.append(&mut pile.forward);
                self.seated[pile.part] += pile.born;
                self.actors_seated += pile.born;
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
            let records: usize = self.lists[self.writing.clone()].iter().map(|lists| lists.routes.records(part)).sum();
            // A part that carried items over has them to run, whether or not anything else reached it.
            let carried = self.streams.at(part).has_items();
            if records > 0 || carried {
                touched.push(part);
            }
        }

        touched
    }

    /// Notes each part with nothing to run as idle a step, and trims the writers' buckets for each that gave back its
    /// room: a part that ran a burst once keeps no room for it while it idles.
    fn rest(&mut self) {
        for part in 0..self.seating.len() {
            let is_idle = !self.has_work(part);
            if is_idle && self.streams.at_mut(part).rest() {
                for lists in &mut self.lists {
                    lists.routes.streams.trim(part, 0);
                }
            }
        }
    }

    /// Whether part `part` has anything to run: an item that has arrived, or a closing turn.
    fn has_work(&self, part: usize) -> bool {
        self.streams.at(part).has_items() || self.lives.at(part).has_ended()
    }

    /// What the turns waiting in part `part` are expected to cost, in nanoseconds.
    fn expect(&self, part: usize) -> u64 {
        fold::expected(self.streams.at(part), self.lives.at(part), &self.seating[part].seated, &self.costs, self.slice)
    }

    /// Takes part `part` of every view out, with every worker's records for it, to fold, into a spare pile.
    fn pile(&mut self, part: usize) -> fold::Pile {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let mut pile = self.spare.piles.pop().unwrap_or_else(|| fold::Pile::empty(parts));
        let empty = Seating { seated: BySeat::empty(parts) };
        let rows = &mut self.lists[self.writing.clone()];
        self.table.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.table), &mut pile.table);
        self.log.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.log), &mut pile.log);
        self.lives.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.lives), &mut pile.lives);
        self.streams.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.streams), &mut pile.streams);
        self.watches.fill(part, rows.iter_mut().map(|lists| &mut lists.routes.watches), &mut pile.watches);
        pile.births.extend(rows.iter_mut().map(|lists| lists.routes.births.take(part)));
        pile.seating = mem::replace(&mut self.seating[part], empty);
        pile.actors = mem::replace(&mut self.actors[part], BySeat::empty(parts));
        pile.part = part;
        pile.expected = 0;
        pile.born = 0;

        pile
    }

    /// Puts back a folded part from `pile`, leaving it empty but for what its watches handed on.
    fn unpile(&mut self, pile: &mut fold::Pile) {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let part = pile.part;
        let rows = &mut self.lists[self.writing.clone()];
        self.table.empty(part, &mut pile.table, rows.iter_mut().map(|lists| &mut lists.routes.table));
        self.log.empty(part, &mut pile.log, rows.iter_mut().map(|lists| &mut lists.routes.log));
        self.lives.empty(part, &mut pile.lives, rows.iter_mut().map(|lists| &mut lists.routes.lives));
        self.streams.empty(part, &mut pile.streams, rows.iter_mut().map(|lists| &mut lists.routes.streams));
        self.watches.empty(part, &mut pile.watches, rows.iter_mut().map(|lists| &mut lists.routes.watches));
        for (row, bucket) in rows.iter_mut().zip(pile.births.drain(..)) {
            row.routes.births.give_back(part, bucket);
        }
        let empty = Seating { seated: BySeat::empty(parts) };
        self.seating[part] = mem::replace(&mut pile.seating, empty);
        self.actors[part] = mem::replace(&mut pile.actors, BySeat::empty(parts));
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
            instances: mem::take(&mut self.instances[part]),
            seats: mem::take(&mut self.seats[part]),
        }
    }

    /// Puts back a part a worker ran.
    fn put(&mut self, parcel: Parcel) {
        let Parcel { part, streams, lives, seating, actors, instances, seats } = parcel;
        self.streams.put(part, streams);
        self.lives.put(part, lives);
        self.seating[part] = seating;
        self.actors[part] = actors;
        self.instances[part] = instances;
        self.seats[part] = seats;
    }

    /// Folds one turn's effects, emitted in `context`, into every view but the streams, and seats the children
    /// it stood up.
    fn fold_turn(&mut self, effects: &[Effect], stood: Vec<Stood>, context: Context) {
        let written = &self.arenas[parity(self.step)];
        self.table.fold(effects, &context);
        self.log.fold(effects, &context, written);
        self.lives.fold(effects, &context, written);

        // A child is stood up after the children its `begin` stood up, so it is seated first, from the back.
        for child in stood.into_iter().rev() {
            let (me, path, above, actor) = child.into_parts();
            self.seat(me, path, above, actor);
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
            births: Births::of_parts(self.seating.len()),
        };

        Lists { routes, ..Lists::default() }
    }

    /// Where the writer numbered `slot` holds the blobs it puts, given `room` bytes to put, taken out to fill.
    fn puts(&mut self, slot: usize, room: usize) -> Puts {
        let engine = self.number.0;
        while self.puts.len() <= slot {
            self.puts.push(Puts::new(engine, self.puts.len(), 0));
        }
        let mut puts = mem::replace(&mut self.puts[slot], Puts::new(engine, slot, 0));
        puts.give_room(room);

        puts
    }

    /// Sums every change to blobs' pins the step made, those made where no worker's buffer was open among them, and
    /// frees each blob left with no pin (ADR-0014).
    fn sweep_blobs(&mut self) {
        pins::take_shared(self.number.0, &mut self.unbuffered);
        self.blobs.change(&mut self.unbuffered);
        let guests = &*self.guests;
        self.blobs.sweep(|freed| guests.unload(freed));
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

    /// Seats the actor `me` names, at `path`, in the seat it was given, and notes it in its parent's seat. It is run
    /// from the next step. A parent is seated before the children it stood up, so its seat is there to note them in.
    fn seat(&mut self, me: Seated, path: ActorPath, above: Above, actor: Box<dyn Runs>) {
        let part = self.part(me.seat);
        self.seated[part] += 1;
        self.actors_seated += 1;
        birth::sit(&mut self.seating[part], &mut self.actors[part], me, path, above, actor);
        if let Above::Actor(parent, _) = above {
            let parent_part = self.part(parent.seat);
            birth::note(&mut self.seating[parent_part], parent, me.id);
        }
    }

    /// The part `seat` falls in.
    fn part(&self, seat: SeatNumber) -> usize {
        part(seat, self.seating.len())
    }

    /// The part with the fewest actors, the first of them when several tie.
    fn emptiest(&self) -> usize {
        (0..self.seated.len()).min_by_key(|part| self.seated[*part]).unwrap_or_default()
    }

    /// How many actors a part may hold before its children go elsewhere: twice what an even spread would give each
    /// part, counting at least one (ADR-0013).
    fn full(&self) -> usize {
        let actors: usize = self.actors_seated;

        2 * (actors / self.seated.len()).max(1)
    }

    /// Gives part `part` its room for the step: how many children it may seat in its own seats. Its seats set aside in
    /// other parts are made up to [`ASIDE`], one in each part with room after it in turn, for its children past its
    /// room. What each part is given depends only on the parts, so a step seats the same children in the same seats
    /// on any number of workers (ADR-0013).
    fn give_room(&mut self, part: usize) {
        let full = self.full();
        let parts = self.seats.len();
        self.seats[part].make_room(full.saturating_sub(self.seated[part]));
        // Each other part in turn, from the one after: a whole turn round them that finds none with room stops.
        let others = parts - 1;
        let mut offset = 0;
        let mut misses = 0;
        while self.seats[part].aside() < ASIDE && misses < others {
            offset = offset % others + 1;
            let other = (part + offset) % parts;
            if self.seated[other] < full {
                let seat = self.seats[other].own();
                self.seats[part].set_aside(seat);
                misses = 0;
            } else {
                misses += 1;
            }
        }
    }

    /// The seats of part `home`, lent to a root's `begin` with every other part's new numbers.
    /// [`Self::take_back_lent`] takes them back.
    fn lend_seats(&mut self, home: usize) -> Seats {
        let room = self.full().saturating_sub(self.seated[home]);
        let mut seats = mem::take(&mut self.seats[home]);
        seats.make_room(room);
        let others: Vec<u64> =
            (0..self.seats.len()).filter(|part| *part != home).map(|part| self.seats[part].own_next()).collect();
        seats.lend(others);

        seats
    }

    /// Takes back the seats lent to a root's `begin` by [`Self::lend_seats`].
    fn take_back_lent(&mut self, home: usize, mut seats: Seats) {
        let lent: Vec<u64> = seats.take_lent().collect();
        let others = (0..self.seats.len()).filter(|part| *part != home);
        for (part, next) in others.zip(lent) {
            self.seats[part].set_next(next);
        }
        seats.make_room(0);
        self.seats[home] = seats;
    }
}

impl Reading<'_> {
    /// Runs a worker's share of a step: in each of its parts, one turn for each item an actor there was handed, and
    /// each closing turn to run, timing each turn by `clock`.
    ///
    /// An actor that closes or fails handles nothing more, so what it was handed behind that turn is dropped.
    pub fn run(&self, share: Share, clock: &impl Clock) -> Done {
        let Share { mut parcels, arena, puts, mut lists } = share;
        let mut tally = mem::take(&mut lists.tally);
        // One list of what turns ask for, over the share: each turn adds to it, and its effects are routed out of it
        // as it finishes.
        let mut asked = Asked::continuing(mem::take(&mut lists.buffers), arena, Seats::default(), puts);
        pins::open(self.engine, mem::take(&mut lists.pins));
        for parcel in &mut parcels {
            self.run_part(parcel.room(), clock, &mut tally, &mut asked, &mut lists);
        }
        let (buffers, _, arena, _, puts) = asked.into_parts();
        lists.buffers = buffers;
        lists.tally = tally;
        lists.pins = pins::close();

        Done { lists, arena, puts, parcels }
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
        lists.fusing.begin(self.hops);
        mem::swap(asked.seats_mut(), room.seats);
        let mut runs = mem::take(&mut lists.runs);
        room.streams.hand(&mut runs);
        for run in &runs {
            let me = Seated { id: ActorId::new(run.bits), seat: SeatNumber::new(run.seat) };
            let is_live = room.lives.life(me).is_some_and(lives::Life::is_live);
            if is_live {
                self.turns(&mut room, me, Work::Items(*run), clock, tally, asked, lists);
            }
        }
        runs.clear();
        lists.runs = runs;

        for me in room.lives.take_ended() {
            self.turns(&mut room, me, Work::Closing, clock, tally, asked, lists);
        }

        // Every item that was waiting has run, so a hop overtakes nothing its sender sent in an earlier step. The hops
        // run in the order they were made, so two from one sender to one receiver are handled in the order sent.
        while let Some(hop) = lists.fusing.next() {
            // A hop to an actor that was cut off in the step would overtake what it carries, so it is carried
            // after that.
            if lists.fusing.carries(hop.to.id) {
                let Hop { to, from, count, root, depth, kind, .. } = hop;
                let envelope = |bytes| Envelope { sender_seat: from.seat, root, depth, kind, bytes };
                room.streams.carry_hop(
                    to,
                    (from.id.bits(), count),
                    envelope,
                    lists.fusing.bytes(&hop),
                    (asked.arena(), hop.sent),
                );
                continue;
            }
            self.hop(&mut room, hop, clock, tally, asked, lists);
        }
        // What each actor kept as the part ran sent is noted once, however many turns it took.
        for (me, stepped) in lists.fusing.kept() {
            let was = room.lives.life(me).map_or(0, lives::Life::sent);
            if stepped.sent > was {
                let (key, record) = lives::sent(me, stepped.sent);
                lists.routes.lives.put(key, record);
            }
        }
        mem::swap(asked.seats_mut(), room.seats);
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
        let Room { streams, lives, seating, actors, instances, .. } = room;
        let (Some(seat), Some(life), Some(held)) = (seating.seated.get(me), lives.life(me), actors.get_mut(me)) else {
            return;
        };
        let Held::Here(mut actor) = mem::replace(held, Held::Away) else {
            return;
        };
        let key = |item| CostKey { actor: seat.kind, item };

        match (work, life.state()) {
            (Work::Closing, State::Ending(ending)) => {
                self.closing(actor, me, seat, life.sent(), ending, clock, tally, asked, instances, lists);
                // Whatever reaches an actor that has gone is dropped when the engine would hand it over.
                seating.seated.remove(me);
                lists.freed.push(me.seat);
                actors.remove(me);
            }
            (Work::Items(run), State::Live) => {
                let sent_before = life.sent();
                let fresh = Stepped { sent: sent_before, spilled: false, ended: false };
                let mut stepped = if lists.fusing.is_open() {
                    lists.fusing.stepped(me, sent_before)
                } else {
                    fresh
                };
                // An actor that sends is kept to its slice, the rest of its items carried over, so what it sends
                // runs on other workers while it goes on; one that sends nothing would only take more steps. Its
                // items are prepared in batches, each the turns between two looks at the clock, so the slice ends
                // only where a batch does.
                let mut slice = Slice::new(self.slice);
                let mut pieces = streams.items(&run);
                let carried = streams.carried();
                let carried_pins = streams.carried_pins();
                let mut is_spent = false;
                while let Some(handed) = pieces.next() {
                    // A batch is prepared once its first item is in hand, so it holds at least one.
                    let begins_batch = slice.taken.is_multiple_of(SLICE_EVERY);
                    if begins_batch {
                        let batch =
                            batch::Waiting { reading: self, first: handed, rest: &pieces, carried, carried_pins };
                        let table = Since {
                            table: self.table,
                            stood: &lists.fusing.stood,
                            sees: Sees::Step,
                            blobs: self.blobs,
                        };
                        let (seats, puts, _) = asked.lent();
                        let reads = Reads { table: &table, made: &seat.made, seats, puts };
                        actor.prepare(Preparing::new(&batch, instances, reads));
                    }
                    // A turn is timed whole, from the item handed to what it asked for routed: what it costs the
                    // worker, beyond what the actor's code takes.
                    tally.time(key(item_kind(handed)), clock, || {
                        let table = Since {
                            table: self.table,
                            stood: &lists.fusing.stood,
                            sees: Sees::Step,
                            blobs: self.blobs,
                        };
                        asked.next_turn();
                        let source = handed.source();
                        let (item, context) = batch::read(handed, self, carried);
                        // The item sits one above what the turn sends in its chain.
                        let chain = Chain { root: context.root, depth: context.depth - 1 };
                        let turning = Turning::new(
                            me,
                            &seat.path,
                            seat.above,
                            &table,
                            &seat.made,
                            asked,
                            instances,
                            stepped.sent,
                            source,
                            chain,
                        );
                        let ran = Ran::of(me, actor.turn(item, turning), asked, context);

                        Self::finish(me, ran, Origin::Waiting, seating, lives, asked, lists, &mut stepped);
                    });
                    if stepped.ended {
                        break;
                    }
                    let has_sent = stepped.sent > sent_before;
                    is_spent = slice.is_spent(clock) && has_sent;
                    if is_spent {
                        break;
                    }
                }
                if is_spent && !stepped.ended {
                    streams.carry(&run, slice.taken, self.mail);
                    lists.fusing.carry(me.id.bits());
                }

                // An actor its part may still fuse to is kept, and what it sent is noted once the part has run.
                if lists.fusing.is_open() {
                    lists.fusing.keep(me, stepped);
                } else if stepped.sent > sent_before {
                    let (key, record) = lives::sent(me, stepped.sent);
                    lists.routes.lives.put(key, record);
                }
                put_back(actors, me, actor);
            }
            // A plan gives a closing turn only to an actor that has ended, and items only to a live one.
            _ => put_back(actors, me, actor),
        }
    }

    /// The closing turn of the actor `me` names, sitting as `sitting` says, which has sent `sent` pieces of mail and
    /// whose life ended as `ending` says: measured into `tally`, what it asks for added to `asked` and routed into
    /// `lists`.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    fn closing(
        &self,
        actor: Box<dyn Runs>,
        me: Seated,
        sitting: &Sitting,
        sent: u64,
        ending: &(How, Context),
        clock: &impl Clock,
        tally: &mut Tally,
        asked: &mut Asked,
        instances: &mut Instances,
        lists: &mut Lists,
    ) {
        let (how, context) = ending;
        // Timed whole, as a turn of a waiting item is.
        tally.time(CostKey { actor: sitting.kind, item: ItemKind::Closing }, clock, || {
            asked.next_turn();
            let why = why(how);
            let closing = Closing::new(me, &sitting.path, sitting.above, self.table, asked, instances, sent, why);
            let ended = actor.end(closing);
            let buffers = asked.buffers_mut();
            // A closing turn that fails asks for nothing: only its log lines are kept, and the engine adds
            // its fault to them (ADR-0009). Either way the engine adds that the actor's life is over, after
            // what the turn sent.
            if let Err(fault) = ended {
                keep_lines(buffers, me);
                buffers.effects.push(Effect::new(me, What::EndFailed(fault)));
            }
            buffers.effects.push(Effect::new(me, What::Ended(how.clone())));
            if lists.fusing.may_hop() {
                let sends = buffers.mailed.iter().filter(|mailed| is_send_by(mailed, me)).count();
                lists.fusing.hops.closing += u64::try_from(sends).unwrap_or(u64::MAX);
            }

            let ran = Ran { stood: Vec::new(), context: *context };
            lists.route(ran, asked, me.id);
        });
    }
}

impl Reading<'_> {
    /// Runs the turn of `hop`, a piece of mail handed straight to its receiver in `room`, measured into `tally`, what
    /// it asks for added to `asked` and routed into `lists` (ADR-0013). Nothing when the receiver is gone or has
    /// ended: the hop is dropped, as mail to it would be.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    fn hop(
        &self,
        room: &mut Room<'_>,
        hop: Hop,
        clock: &impl Clock,
        tally: &mut Tally,
        asked: &mut Asked,
        lists: &mut Lists,
    ) {
        let Room { lives, seating, actors, instances, .. } = room;
        let me = hop.to;
        let (Some(seat), Some(life), Some(held)) = (seating.seated.get(me), lives.life(me), actors.get_mut(me)) else {
            return;
        };
        let mut stepped = lists.fusing.stepped(me, life.sent());
        let is_live = life.is_live() && !stepped.ended;
        if !is_live {
            return;
        }
        let Held::Here(mut actor) = mem::replace(held, Held::Away) else {
            return;
        };

        let table = Since {
            table: self.table,
            stood: &lists.fusing.stood,
            sees: Sees::Run(lists.fusing.run(hop.run)),
            blobs: self.blobs,
        };
        let (seats, puts, arena) = asked.lent();
        let reads = Reads { table: &table, made: &seat.made, seats, puts };
        let batch = batch::OneHop { reading: self, hop: &hop, lists, arena };
        actor.prepare(Preparing::new(&batch, instances, reads));
        let sent_before = stepped.sent;
        let key = CostKey { actor: seat.kind, item: ItemKind::Mail(hop.kind) };
        // Timed whole, as a turn of a waiting item is.
        tally.time(key, clock, || {
            asked.next_turn();
            let handled = {
                let table = Since {
                    table: self.table,
                    stood: &lists.fusing.stood,
                    sees: Sees::Run(lists.fusing.run(hop.run)),
                    blobs: self.blobs,
                };
                let bytes = lists.fusing.bytes(&hop);
                let mail = Mail::new(hop.kind, bytes, hop.from, hop.root, hop.depth, self.read_limit);
                let chain = Chain { root: hop.root, depth: hop.depth };
                let turning = Turning::new(
                    me,
                    &seat.path,
                    seat.above,
                    &table,
                    &seat.made,
                    asked,
                    instances,
                    sent_before,
                    hop.from,
                    chain,
                );

                actor.turn(Item::Mail(mail), turning)
            };
            let context = Context { root: hop.root, depth: hop.depth + 1 };
            let ran = Ran::of(me, handled, asked, context);
            Self::finish(me, ran, Origin::Hop(hop.run), seating, lives, asked, lists, &mut stepped);
        });

        lists.fusing.keep(me, stepped);
        put_back(actors, me, actor);
    }

    /// Routes the finished turn `ran` of the actor `me` names into `lists`, and notes in `stepped` what it did: the
    /// mail it sent and whether it ended (ADR-0013).
    ///
    /// Its first send becomes the part's next hop when the part takes one, the actor has spilled no mail to its part,
    /// and the receiver sits in `seating` and is live in `lives`. A turn of a hop, and the turn that made the part's
    /// first hop, are its run, so what they stood up and ended is noted in the run.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    #[allow(clippy::inline_always, reason = "once for every turn: a call here costs a tenth of a turn, measured")]
    #[inline(always)]
    fn finish(
        me: Seated,
        ran: Ran,
        origin: Origin,
        seating: &Seating,
        lives: &lives::Shard,
        asked: &mut Asked,
        lists: &mut Lists,
        stepped: &mut Stepped,
    ) {
        // Most turns stand up no child. What a child sent from its `begin` is put once the turn is routed, behind the
        // spawn that makes its life: the view drops a count for an actor it does not hold.
        let begun_sent = if ran.stood.is_empty() {
            Vec::new()
        } else {
            let mailed = &asked.buffers_mut().mailed;

            senders(mailed).into_iter().filter(|(actor, _)| actor.id != me.id).collect()
        };
        if !ran.stood.is_empty() {
            let stood = ran
                .stood
                .iter()
                .map(|child| (child.id().bits(), (child.path().clone(), child.seated().seat, child.declared())));
            lists.fusing.stood.extend(stood);
        }

        let mut run = match origin {
            Origin::Waiting => None,
            Origin::Hop(run) => Some(run),
        };
        // Once the budget is spent no hop is made, so nothing that decides one need be looked at.
        if lists.fusing.may_hop() {
            let mailed = &asked.buffers_mut().mailed;
            let mut sends = (0..mailed.len()).filter(|index| is_send_by(&mailed[*index], me));
            let first = sends.next();
            let burst = sends.count();
            if let Some(index) = first {
                let Mailed { to, count, kind, bytes, .. } = mailed[index];
                let hand = hand(to, *stepped, seating, lives, &lists.fusing);
                lists.fusing.hops.count(hand);
                let pieces = bytes.pieces();
                let is_run = pieces > 1;
                lists.fusing.hops.burst += u64::try_from(burst).unwrap_or(u64::MAX) + u64::from(pieces) - 1;
                if hand == Hand::Fuse {
                    let first = if is_run {
                        bytes.piece(0, bytes.each())
                    } else {
                        bytes
                    };
                    let (at, len) = lists.fusing.keep_bytes(asked.arena().read(first).unwrap_or_default());
                    let mailed = &mut asked.buffers_mut().mailed;
                    // Of a run, the first piece is fused and the rest stays mail. The fused send is most often the
                    // turn's last piece of mail, and nothing is moved for it.
                    if is_run && let Some(rest) = bytes.rest() {
                        mailed[index].count += 1;
                        mailed[index].bytes = rest;
                    } else if index + 1 == mailed.len() {
                        mailed.pop();
                    } else {
                        mailed.remove(index);
                    }
                    let context = ran.context;
                    // A fused send from a waiting item begins a run; one from a hop goes on with the hop's.
                    let chain = *run.get_or_insert_with(|| lists.fusing.begin_run());
                    let hop = Hop {
                        to,
                        from: me,
                        count,
                        root: context.root,
                        depth: context.depth,
                        kind,
                        run: chain,
                        at,
                        len,
                        sent: first,
                    };
                    lists.fusing.wait(hop);
                    stepped.sent = stepped.sent.max(count);
                }
            }
            if !stepped.spilled {
                let mailed = &asked.buffers_mut().mailed;
                stepped.spilled = mailed.iter().any(|mailed| send_into(mailed, me, seating));
            }
        }

        if let Some(run) = run
            && !ran.stood.is_empty()
        {
            let stood = ran.stood.iter().map(|child| (child.seated(), child.path().clone()));
            lists.fusing.run_mut(run).stand(stood);
        }
        // Most turns of a busy actor ask for nothing, and then there is nothing to route.
        let asked_nothing = asked.asked_nothing() && ran.stood.is_empty();
        let seen = if asked_nothing {
            Seen::default()
        } else {
            lists.route(ran, asked, me.id)
        };
        for (child, sent) in begun_sent {
            let (key, record) = lives::sent(child, sent);
            lists.routes.lives.put(key, record);
        }
        stepped.sent = seen.sent.map_or(stepped.sent, |sent| sent.max(stepped.sent));
        if seen.ends {
            stepped.ended = true;
            if let Some(run) = run {
                lists.fusing.run_mut(run).end(me.id);
            }
        }
    }
}

/// What a turn ran for: an item that was waiting as the step began, or a hop made in the step (ADR-0013).
#[derive(Clone, Copy)]
enum Origin {
    Waiting,
    /// A hop of the run numbered so in its part's step.
    Hop(usize),
}

/// What becomes of the first send of a turn, by an actor that has done `stepped` in the step, to `to`, while its
/// part may still fuse: it is fused when `to` sits in the part, in `seating`, live in `lives` and not ended in the
/// step nor cut off at its slice, and its sender has sent no mail as mail into its part in the step (ADR-0013). What `to` is decides first, so
/// a send the spill rule held back is one that could have been fused without it.
///
/// Each rule is looked at only once the ones before it have passed, so a send to another part costs no lookup of
/// what was done in the step.
fn hand(to: Seated, stepped: Stepped, seating: &Seating, lives: &lives::Shard, fusing: &Fusing) -> Hand {
    let was_stood = !fusing.stood.is_empty() && fusing.stood.contains_key(&to.id.bits());
    if was_stood {
        return Hand::Unseated;
    }
    let is_seated = seating.seated.get(to).is_some();
    if !is_seated {
        return Hand::Elsewhere;
    }
    let is_live = lives.life(to).is_some_and(lives::Life::is_live);
    let is_over = !is_live || fusing.stepped(to, 0).ended;
    if is_over {
        return Hand::Ended;
    }

    if stepped.spilled || fusing.carries(to.id) {
        Hand::Spilled
    } else {
        Hand::Fuse
    }
}

/// Whether `mailed` is mail the actor `me` sent.
fn is_send_by(mailed: &Mailed, me: Seated) -> bool {
    mailed.by.id == me.id
}

/// Whether `mailed` is mail the actor `me` sent to an actor seated in `seating`, its own part.
fn send_into(mailed: &Mailed, me: Seated, seating: &Seating) -> bool {
    let is_own = mailed.by.id == me.id;

    is_own && seating.seated.get(mailed.to).is_some()
}

impl Parcel {
    /// The part, for its turns to change.
    fn room(&mut self) -> Room<'_> {
        Room {
            streams: &mut self.streams,
            lives: &mut self.lives,
            seating: &mut self.seating,
            actors: &mut self.actors,
            instances: &mut self.instances,
            seats: &mut self.seats,
        }
    }
}

impl Lists {
    /// Routes a finished turn's effects into this worker's row of buckets for each view, taking them out of the
    /// list, routes the children the turn stood up to the parts that seat them, and answers with what it saw of the
    /// actor `me`.
    ///
    /// Each effect is routed into every view that keeps anything of it in one pass: the other views borrow it, then
    /// the streams take it by value. A turn that stood up a child whose `begin` failed is routed without what that
    /// child left.
    #[allow(clippy::inline_always, reason = "once for every turn: a call here costs a tenth of a turn, measured")]
    #[inline(always)]
    fn route(&mut self, ran: Ran, asked: &mut Asked, me: ActorId) -> Seen {
        let Ran { stood, context } = ran;
        let routes = &mut self.routes;
        let buffers = asked.buffers_mut();
        let mut seen = Seen::default();
        // Each buffer goes to the views that read it: the lines to the log, ahead of the fault a failed turn ends
        // with; the effects to every view that keeps something of them; the mail to the streams alone.
        for line in buffers.lines.drain(..) {
            let (key, record) = log::line(line);
            routes.log.put(key, record);
        }
        for effect in buffers.effects.drain(..) {
            seen.see(&effect, me);
            routes.route(&effect, &context);
        }
        for mailed in buffers.mailed.drain(..) {
            seen.sent(&mailed, me);
            let (key, entry) = stream::entry(mailed, &context);
            routes.streams.put(key, entry);
        }

        // Most turns stand up no child.
        if !stood.is_empty() {
            self.routes.births.stand(stood);
        }
        seen
    }
}

impl Routes {
    /// How many records this row holds for part `part`, across the views.
    fn records(&self, part: usize) -> usize {
        let views = self.table.records(part) + self.log.records(part) + self.lives.records(part);

        views + self.streams.records(part) + self.watches.records(part) + self.births.records(part)
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
    /// The turn of the actor `me` names that returned `outcome`, having asked for what is in `asked` since its turn began,
    /// in `context`.
    ///
    /// A turn that fails changes nothing outside its actor: what it asked for is dropped but for its log lines,
    /// and the engine writes the one effect that says the actor ended (ADR-0009). The seats of the children it
    /// stood up are given back.
    #[allow(clippy::inline_always, reason = "once for every turn: a call here costs a tenth of a turn, measured")]
    #[inline(always)]
    fn of(me: Seated, outcome: Result<(), Fault>, asked: &mut Asked, context: Context) -> Self {
        let mut stood = asked.take_stood();
        if let Err(fault) = outcome {
            keep_lines(asked.buffers_mut(), me);
            asked.buffers_mut().effects.push(Effect::new(me, What::Failed(fault)));
            for child in mem::take(&mut stood) {
                asked.seats_mut().give_back(child.seated().seat);
            }
        }

        Self { stood, context }
    }
}

/// Puts the actor `me` names back in its seat in `actors` after its turns.
fn put_back(actors: &mut BySeat<Held>, me: Seated, actor: Box<dyn Runs>) {
    if let Some(held) = actors.get_mut(me) {
        *held = Held::Here(actor);
    }
}

/// Each actor that sent mail among `mailed`, with the count of the last piece it sent.
fn senders(mailed: &[Mailed]) -> Vec<(Seated, u64)> {
    let mut senders: Vec<(Seated, u64)> = Vec::new();
    for &Mailed { by, count, bytes, .. } in mailed {
        let count = &(count + u64::from(bytes.pieces()) - 1);
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
        if is_own {
            self.ends |= ending(effect, me).is_some();
        }
    }

    /// Notes `mailed`, when the actor `me` sent it.
    fn sent(&mut self, mailed: &Mailed, me: ActorId) {
        let is_own = mailed.by.id == me;
        if is_own {
            let last = mailed.count + u64::from(mailed.bytes.pieces()) - 1;
            self.sent = Some(self.sent.map_or(last, |sent| sent.max(last)));
        }
    }
}

/// Drops all the turn of the actor `me` names asked for but the lines it logged and the failures of other actors its
/// host reported, which only a hosted turn whose instance trapped writes: what is kept of a turn that failed
/// (ADR-0009, ADR-0015).
fn keep_lines(buffers: &mut Buffers, me: Seated) {
    buffers.mailed.clear();
    buffers.effects.retain(|effect| is_reported_failure(effect, me));
}

/// Whether `effect` is the failure of an actor other than `me` that its host reported.
fn is_reported_failure(effect: &Effect, me: Seated) -> bool {
    let is_failure = matches!(effect.what(), What::Failed(_));
    let is_other = effect.by() != me.id;

    is_failure && is_other
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

/// How many seats in other parts a part is given in each step it runs, for its children past its room.
const ASIDE: usize = 8;

/// Which of the two arenas of each writer step `step` writes.
const fn parity(step: u64) -> usize {
    (step % 2) as usize
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

/// How many turns an actor kept to a slice runs between looks at the clock: the most items a batch holds.
const SLICE_EVERY: u32 = 32;

/// How far one actor has gone into its slice of a step: how many turns it has taken, and when its slice began.
struct Slice {
    /// The most nanoseconds its turns take: none past `u64::MAX`, which keeps it to no slice.
    nanos: u64,
    taken: u32,
    began: u64,
}

impl Slice {
    /// An actor's slice of `nanos` nanoseconds, before it has taken a turn.
    const fn new(nanos: u64) -> Self {
        Self { nanos, taken: 0, began: 0 }
    }

    /// Counts a turn taken, and answers whether the actor has run past its slice. The clock is read every
    /// [`SLICE_EVERY`] turns, at the end of a batch, and the slice timed from the first read, so an actor with few
    /// items reads it never.
    fn is_spent(&mut self, clock: &impl Clock) -> bool {
        self.taken += 1;
        let is_kept = self.nanos != u64::MAX;
        let is_checked = is_kept && self.taken.is_multiple_of(SLICE_EVERY);
        if !is_checked {
            return false;
        }
        let now = clock.nanos();
        if self.taken == SLICE_EVERY {
            self.began = now;
        }

        now.saturating_sub(self.began) >= self.nanos
    }
}

/// How many times what splitting a step among every worker costs an actor kept to a slice runs before its items are
/// carried over.
const SLICE_SPLITS: u64 = 16;
/// The least an actor kept to a slice runs, in nanoseconds, on a host that measured no cost of splitting.
const SLICE_LEAST: u64 = 50_000;
/// How many carried items of one actor the plan walks to weigh them: a slice's worth for any but the cheapest items.
const CARRIED_WEIGHED: u64 = 4096;
