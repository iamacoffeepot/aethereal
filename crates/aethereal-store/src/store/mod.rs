//! The store's root (ADR-0039).

mod shelves;

use crate::jobs::{Counted, Counting, Recount, Shelve, Shelved, Shelving, Stock, Stocked, Stocking};
use crate::{
    CheckIn, CheckedIn, Held, Hold, ListStored, Pin, Pinned, StoreConfig, StoreHash, StoreName, Stored, Unpin,
};
use aethereal_actor::Held as Debt;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Carries, ChildOf, Ended, Ending, ErasedActorRef, Fault, Handles, Level,
    ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use shelves::{Admitted, Shelves};
use std::collections::{BTreeSet, VecDeque};
use std::mem;

/// The store: keeps binaries checked in from a path on the host's machine, each named by the BLAKE3 hash of its bytes,
/// under names that move to the latest build, pruned to a byte budget (ADR-0039). An actor asks it at its typed path,
/// `path::<Store>(())`: [`CheckIn`], [`ListStored`], [`Pin`], [`Unpin`], and [`Hold`]. Its bytes are never given back.
///
/// It reads its index from its host's stockroom as it begins, holding what it is asked until it has. It runs one job
/// of its stockroom at a time, so a removal is done before a later copy of the same bytes, and writes land in the
/// order they were made: check-ins wait their turn, and the records each change dirties are written together once the
/// job before is done.
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
}

/// A check-in waiting for its copy.
#[derive(Bound)]
struct Waiting {
    debt: Debt<CheckedIn>,
    check_in: CheckIn,
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
    /// The check-ins waiting for the lane, in the order asked.
    queue: VecDeque<Waiting>,
    unwritten: Unwritten,
}

impl Open {
    /// Answers `ask`, or queues it for the lane when it is a check-in.
    fn take<S>(&mut self, ask: Ask, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        match ask {
            Ask::CheckIn { debt, check_in } => self.queue.push_back(Waiting { debt, check_in }),
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
        }

        Ok(())
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
    /// later copy of the same bytes, then the next check-in.
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
        let Some(Waiting { debt, check_in: CheckIn { path, name } }) = self.queue.pop_front() else {
            return Ok(());
        };
        turn.run(&Stock { path, most_file_bytes: config.most_file_bytes }, &Stocking)?;
        self.lane = Lane::Stocking { debt, name };

        Ok(())
    }

    /// Answers the check-in whose copy came to `stocked`, fitting it into `most_bytes`.
    fn stocked<S>(&mut self, stocked: Stocked, most_bytes: u64, turn: &mut Turn<'_, Store, S>) -> Result<(), Fault> {
        let (debt, name) = match mem::replace(&mut self.lane, Lane::Idle) {
            Lane::Stocking { debt, name } => (debt, name),
            // A copy is answered only while it runs, so this is never another job's lane.
            running @ (Lane::Idle | Lane::Shelving) => {
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

    /// Fits the copy of `hash` into the index, noting what it changed to write.
    fn admit(&mut self, hash: StoreHash, bytes: u64, name: StoreName, most_bytes: u64) -> CheckedIn {
        match self.shelves.admit(hash, bytes, name, most_bytes) {
            Admitted::Stored { fresh, pruned } => {
                self.unwritten.record(hash);
                for gone in pruned {
                    self.unwritten.prune(gone);
                }
                CheckedIn::Stored { hash, bytes, fresh }
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

impl Actor for Store {
    type Config = StoreConfig;
    type Key = ();
    type Handles = (
        CheckIn,
        ListStored,
        Pin,
        Unpin,
        Hold,
        Ended,
        Answer<Counted, Counting>,
        Answer<Stocked, Stocking>,
        Answer<Shelved, Shelving>,
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

/// A holder has ended: its holds go. Nothing is held while the index is read, as a hold is taken only once it is.
impl Handles<Ended> for Store {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        if let Stage::Open { open } = &mut self.stage {
            open.shelves.ended(ended.actor());
        }

        Ok(())
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
