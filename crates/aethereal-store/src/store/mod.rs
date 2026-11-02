//! The store's root (ADR-0039).

mod launches;
mod shelves;

use crate::jobs::{
    Counted, Counting, Issue, Issued, Issuing, Recall, Recalled, Recalling, Recount, Shelve, Shelved, Shelving, Stock,
    Stocked, Stocking,
};
use crate::{
    Binary, CheckIn, CheckedIn, Held, Hold, Launch, Launched, ListStored, Pin, Pinned, StoreConfig, StoreHash,
    StoreName, Stored, StoredBinary, Unpin,
};
use aethereal_actor::Held as Debt;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Carries, ChildOf, Ended, Ending, ErasedActorRef, Fault, Handles, Level,
    ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use launches::Launches;
use shelves::{Admitted, Launchable, Shelves};
use std::collections::{BTreeSet, VecDeque};
use std::mem;

/// The store: keeps binaries checked in from a path on the host's machine, each named by the BLAKE3 hash of its bytes,
/// under names that move to the latest build, pruned to a byte budget (ADR-0039). An actor asks it at its typed path,
/// `path::<Store>(())`: [`CheckIn`], [`ListStored`], [`Pin`], [`Unpin`], [`Hold`], and [`Launch`] (ADR-0040). Its bytes
/// are never given back; a launch copy of an entry is, to run, removed when its asker ends.
///
/// It reads its index from its host's stockroom as it begins, holding what it is asked until it has. It runs one job
/// of its stockroom at a time, so a removal is done before a later copy of the same bytes, and writes land in the
/// order they were made: check-ins, launch copies, and their removals wait their turn, and the records each change
/// dirties are written together once the job before is done.
#[derive(Bound)]
#[kind(name = "aether.store")]
pub struct Store {
    config: StoreConfig,
    stage: Stage,
}

/// Why the store failed: it could not read its index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.uncounted")]
pub struct Uncounted;

/// Where the store is in beginning.
#[derive(Bound)]
enum Stage {
    /// Its index is being read: what it was asked, in the order asked, waits for it.
    Counting { asks: Vec<Ask> },
    /// Its index is read.
    Open { open: Box<Open> },
}

/// A request the store holds, with what it asked.
#[derive(Bound)]
enum Ask {
    CheckIn { debt: Debt<CheckedIn>, check_in: CheckIn },
    List { debt: Debt<Stored> },
    Pin { debt: Debt<Pinned>, hash: StoreHash },
    Unpin { debt: Debt<Pinned>, hash: StoreHash },
    Hold { debt: Debt<Held>, hash: StoreHash, holder: ErasedActorRef },
    Launch { debt: Debt<Launched>, binary: Binary, asker: ErasedActorRef },
}

/// A job of the stockroom waiting for the lane.
#[derive(Bound)]
enum Waiting {
    /// A check-in, waiting for its copy.
    CheckIn { debt: Debt<CheckedIn>, check_in: CheckIn },
    /// A launch of the entry `hash`, held already, waiting for its launch copy `copy`.
    Launch { debt: Debt<Launched>, hash: StoreHash, copy: String },
    /// The launch copies of an asker that has ended, waiting to be removed.
    Recall { copies: Vec<String> },
}

/// The job of the stockroom the store is running.
#[derive(Bound)]
enum Lane {
    /// None.
    Idle,
    /// The copy of the check-in to `name`, which `debt` is answered with.
    Stocking { debt: Debt<CheckedIn>, name: StoreName },
    /// A write.
    Shelving,
    /// The launch copy of `hash`, which `debt` is answered with.
    Issuing { debt: Debt<Launched>, hash: StoreHash },
    /// A removal of launch copies.
    Recalling,
}

/// What the store has changed and not yet written.
#[derive(Bound)]
enum Unwritten {
    /// Nothing.
    Clean,
    /// The records of `records`, the removal of `prune`, and the names file.
    Due { records: BTreeSet<StoreHash>, prune: BTreeSet<StoreHash> },
}

impl Unwritten {
    /// Notes that the record of `hash` changed.
    fn record(&mut self, hash: StoreHash) {
        match self {
            Self::Clean => *self = Self::Due { records: BTreeSet::from([hash]), prune: BTreeSet::new() },
            Self::Due { records, .. } => {
                records.insert(hash);
            }
        }
    }

    /// Notes that `hash` is to be removed: its record is not written.
    fn prune(&mut self, hash: StoreHash) {
        match self {
            Self::Clean => *self = Self::Due { records: BTreeSet::new(), prune: BTreeSet::from([hash]) },
            Self::Due { records, prune } => {
                records.remove(&hash);
                prune.insert(hash);
            }
        }
    }
}

/// The store once its index is read.
#[derive(Bound)]
struct Open {
    shelves: Shelves,
    lane: Lane,
    /// The jobs waiting for the lane, in the order asked.
    queue: VecDeque<Waiting>,
    unwritten: Unwritten,
    launches: Launches,
}

impl Open {
    /// Answers `ask`, or queues it for the lane when it is a check-in.
    fn take<S>(&mut self, ask: Ask, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        match ask {
            Ask::CheckIn { debt, check_in } => self.queue.push_back(Waiting::CheckIn { debt, check_in }),
            Ask::List { debt } => turn.answer(debt, &self.shelves.stored())?,
            Ask::Pin { debt, hash } => self.pin(debt, hash, true, turn)?,
            Ask::Unpin { debt, hash } => self.pin(debt, hash, false, turn)?,
            Ask::Hold { debt, hash, holder } => {
                let held = self.shelves.hold(hash, holder);
                if held == Held::Done {
                    turn.watch(holder);
                    self.unwritten.record(hash);
                }
                turn.answer(debt, &held)?;
            }
            Ask::Launch { debt, binary, asker } => self.launch(debt, &binary, asker, turn)?,
        }

        Ok(())
    }

    /// Holds the entry `binary` names for `asker`, and queues its launch copy for the lane; or answers `debt` that
    /// it is not stored.
    fn launch<S>(
        &mut self,
        debt: Debt<Launched>,
        binary: &Binary,
        asker: ErasedActorRef,
        turn: &mut Turn<'_, Store, S>,
    ) -> Result<(), Fault> {
        let Launchable::Held { hash, name } = self.shelves.launch(binary, asker) else {
            turn.answer(debt, &Launched::NotStored)?;
            return Ok(());
        };
        turn.watch(asker);
        self.unwritten.record(hash);
        let copy = self.launches.hand(asker, &name);
        self.queue.push_back(Waiting::Launch { debt, hash, copy });

        Ok(())
    }

    /// Lets go of what `actor`, which has ended, held: its holds, and its launch copies, queued for removal.
    fn ended(&mut self, actor: ErasedActorRef) {
        self.shelves.ended(actor);
        let copies = self.launches.ended(actor);
        if !copies.is_empty() {
            self.queue.push_back(Waiting::Recall { copies });
        }
    }

    /// Pins `hash`, or clears its pin, and answers `debt`.
    fn pin<S>(
        &mut self,
        debt: Debt<Pinned>,
        hash: StoreHash,
        pinned: bool,
        turn: &mut Turn<'_, Store, S>,
    ) -> Result<(), Fault> {
        let answer = self.shelves.set_pin(hash, pinned);
        if answer == Pinned::Done {
            self.unwritten.record(hash);
        }
        turn.answer(debt, &answer)?;

        Ok(())
    }

    /// Runs the next job of the stockroom when none is running: what is unwritten first, so a removal lands before a
    /// later copy of the same bytes, then the next job waiting.
    fn pump<S>(&mut self, config: StoreConfig, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        let is_busy = !matches!(self.lane, Lane::Idle);
        if is_busy {
            return Ok(());
        }
        if let Unwritten::Due { records, prune } = mem::replace(&mut self.unwritten, Unwritten::Clean) {
            let shelve = Shelve {
                prune: prune.into_iter().collect(),
                records: self.shelves.records(&records),
                names: self.shelves.names(),
            };
            turn.run(&shelve, &Shelving)?;
            self.lane = Lane::Shelving;
            return Ok(());
        }
        let Some(waiting) = self.queue.pop_front() else {
            return Ok(());
        };
        let lane = match waiting {
            Waiting::CheckIn { debt, check_in: CheckIn { path, name } } => {
                turn.run(&Stock { path, most_file_bytes: config.most_file_bytes }, &Stocking)?;
                Lane::Stocking { debt, name }
            }
            Waiting::Launch { debt, hash, copy } => {
                turn.run(&Issue { hash, copy }, &Issuing)?;
                Lane::Issuing { debt, hash }
            }
            Waiting::Recall { copies } => {
                turn.run(&Recall { copies }, &Recalling)?;
                Lane::Recalling
            }
        };
        self.lane = lane;

        Ok(())
    }

    /// Answers the check-in whose copy came to `stocked`, fitting it into `most_bytes`.
    fn stocked<S>(&mut self, stocked: Stocked, most_bytes: u64, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        let (debt, name) = match mem::replace(&mut self.lane, Lane::Idle) {
            Lane::Stocking { debt, name } => (debt, name),
            // A copy is answered only while it runs, so this is never another job's lane.
            running @ (Lane::Idle | Lane::Shelving | Lane::Issuing { .. } | Lane::Recalling) => {
                self.lane = running;
                return Ok(());
            }
        };
        let checked_in = match stocked {
            Stocked::Copied { hash, bytes } => self.admit(hash, bytes, name, most_bytes),
            Stocked::Missing => CheckedIn::Missing,
            Stocked::NotAFile => CheckedIn::NotAFile,
            Stocked::TooLarge => CheckedIn::TooLarge,
            Stocked::Failed => CheckedIn::Failed,
        };
        turn.answer(debt, &checked_in)?;

        Ok(())
    }

    /// Answers the launch whose copy came to `issued`.
    fn issued<S>(&mut self, issued: Issued, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        let (debt, hash) = match mem::replace(&mut self.lane, Lane::Idle) {
            Lane::Issuing { debt, hash } => (debt, hash),
            // A launch copy is answered only while it is made, so this is never another job's lane.
            running @ (Lane::Idle | Lane::Stocking { .. } | Lane::Shelving | Lane::Recalling) => {
                self.lane = running;
                return Ok(());
            }
        };
        let launched = match issued {
            Issued::Ready { program } => Launched::Ready { program, hash },
            Issued::Failed => Launched::Failed,
        };
        turn.answer(debt, &launched)?;

        Ok(())
    }

    /// Fits the copy of `hash` into the index, noting what it changed to write.
    fn admit(&mut self, hash: StoreHash, bytes: u64, name: StoreName, most_bytes: u64) -> CheckedIn {
        match self.shelves.admit(hash, bytes, name, most_bytes) {
            Admitted::Stored { fresh, pruned } => {
                self.unwritten.record(hash);
                for gone in pruned {
                    self.unwritten.prune(gone);
                }
                CheckedIn::Stored(StoredBinary { hash, bytes, fresh })
            }
            Admitted::Full => {
                // The copy was new bytes, now in the stockroom and in no entry.
                self.unwritten.prune(hash);
                CheckedIn::Full
            }
        }
    }
}

impl Store {
    /// Answers `ask` once the index is read, or keeps it until then.
    fn take<S>(&mut self, ask: Ask, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        match &mut self.stage {
            Stage::Counting { asks } => asks.push(ask),
            Stage::Open { open } => {
                open.take(ask, turn)?;
                open.pump(self.config, turn)?;
            }
        }

        Ok(())
    }
}

impl ChildOf for Store {
    type Parent = Top;
}

/// Asked at its typed path, it answers a check-in once its copy is done.
impl Carries<CheckIn> for Store {
    type Answers = (CheckedIn,);
}

/// Asked at its typed path, it lists what it holds.
impl Carries<ListStored> for Store {
    type Answers = (Stored,);
}

/// Asked at its typed path, it pins an entry.
impl Carries<Pin> for Store {
    type Answers = (Pinned,);
}

/// Asked at its typed path, it clears an entry's pin.
impl Carries<Unpin> for Store {
    type Answers = (Pinned,);
}

/// Asked at its typed path, it holds an entry while the asker lives.
impl Carries<Hold> for Store {
    type Answers = (Held,);
}

/// Asked at its typed path, it answers a launch once its launch copy is made.
impl Carries<Launch> for Store {
    type Answers = (Launched,);
}

impl Actor for Store {
    type Config = StoreConfig;
    type Key = ();
    type Handles = (
        CheckIn,
        ListStored,
        Pin,
        Unpin,
        Hold,
        Launch,
        Ended,
        Answer<Counted, Counting>,
        Answer<Stocked, Stocking>,
        Answer<Shelved, Shelving>,
        Answer<Issued, Issuing>,
        Answer<Recalled, Recalling>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: StoreConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.run(&Recount, &Counting)?;

        Ok(Self { config, stage: Stage::Counting { asks: Vec::new() } })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<CheckIn> for Store {
    type Sends = (ToSender<CheckedIn>,);

    fn handle(&mut self, check_in: CheckIn, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        self.take(Ask::CheckIn { debt, check_in }, turn)
    }
}

impl Handles<ListStored> for Store {
    type Sends = (ToSender<Stored>,);

    fn handle(&mut self, _: ListStored, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        self.take(Ask::List { debt }, turn)
    }
}

impl Handles<Pin> for Store {
    type Sends = (ToSender<Pinned>,);

    fn handle(&mut self, Pin { hash }: Pin, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        self.take(Ask::Pin { debt, hash }, turn)
    }
}

impl Handles<Unpin> for Store {
    type Sends = (ToSender<Pinned>,);

    fn handle(&mut self, Unpin { hash }: Unpin, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        self.take(Ask::Unpin { debt, hash }, turn)
    }
}

impl Handles<Hold> for Store {
    type Sends = (ToSender<Held>,);

    fn handle(&mut self, Hold { hash }: Hold, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        let holder = turn.sender();
        self.take(Ask::Hold { debt, hash, holder }, turn)
    }
}

impl Handles<Launch> for Store {
    type Sends = (ToSender<Launched>,);

    fn handle(&mut self, Launch { binary }: Launch, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        let asker = turn.sender();
        self.take(Ask::Launch { debt, binary, asker }, turn)
    }
}

/// A holder has ended: its holds go, and its launch copies are removed. Nothing is held and no launch copy handed out
/// while the index is read, as each is taken only once it is.
impl Handles<Ended> for Store {
    type Sends = ();

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Stage::Open { open } = &mut self.stage else {
            return Ok(());
        };
        open.ended(ended.actor());
        open.pump(self.config, turn)
    }
}

impl Handles<Answer<Counted, Counting>> for Store {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Counting): (Answered<Counted>, Counting),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Counted::Read { entries, names }) = answered else {
            turn.log(Level::Error, format_args!("the store's index was not read: {answered:?}"));
            return Err(Fault::new(&Uncounted)?);
        };
        let open = Open {
            shelves: Shelves::rebuilt(entries, names),
            lane: Lane::Idle,
            queue: VecDeque::new(),
            unwritten: Unwritten::Clean,
            launches: Launches::default(),
        };
        let Stage::Counting { asks } = mem::replace(&mut self.stage, Stage::Open { open: Box::new(open) }) else {
            return Ok(());
        };
        for ask in asks {
            self.take(ask, turn)?;
        }

        Ok(())
    }
}

impl Handles<Answer<Stocked, Stocking>> for Store {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Stocking): (Answered<Stocked>, Stocking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Stage::Open { open } = &mut self.stage else {
            return Ok(());
        };
        let stocked = match answered {
            Answered::Yes(stocked) => stocked,
            Answered::No(_) => Stocked::Failed,
        };
        open.stocked(stocked, self.config.most_bytes, turn)?;
        open.pump(self.config, turn)
    }
}

impl Handles<Answer<Shelved, Shelving>> for Store {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Shelving): (Answered<Shelved>, Shelving),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Stage::Open { open } = &mut self.stage else {
            return Ok(());
        };
        open.lane = Lane::Idle;
        let is_done = matches!(answered, Answered::Yes(Shelved::Done));
        if !is_done {
            // The index stands. A record the stockroom missed is set right by its next write, and an entry it did not
            // remove is found by the recount as the store next begins, unnamed, to be pruned again.
            turn.log(Level::Warn, format_args!("the store's stockroom was not all written: {answered:?}"));
        }
        open.pump(self.config, turn)
    }
}

impl Handles<Answer<Issued, Issuing>> for Store {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Issuing): (Answered<Issued>, Issuing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Stage::Open { open } = &mut self.stage else {
            return Ok(());
        };
        let issued = match answered {
            Answered::Yes(issued) => issued,
            Answered::No(_) => Issued::Failed,
        };
        open.issued(issued, turn)?;
        open.pump(self.config, turn)
    }
}

impl Handles<Answer<Recalled, Recalling>> for Store {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Recalling): (Answered<Recalled>, Recalling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Stage::Open { open } = &mut self.stage else {
            return Ok(());
        };
        open.lane = Lane::Idle;
        let is_done = matches!(answered, Answered::Yes(Recalled::Done));
        if !is_done {
            // A launch copy left behind is removed as the store next begins.
            turn.log(Level::Warn, format_args!("the store's launch copies were not all removed: {answered:?}"));
        }
        open.pump(self.config, turn)
    }
}
