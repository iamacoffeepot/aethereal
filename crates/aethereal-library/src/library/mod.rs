//! The library root: the engine's one publisher, the head of every kind it has published, and where it stands as the
//! program starts (ADR-0054).

mod defaults;
mod publish;
mod start;

use crate::{Defaulted, Defaults, ModulePublished, PublishModule};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Bytes, Carries, ChildOf, Ending, Fault, Given, Handle, Handles, Held,
    Path, Publisher, Refused, Top, Turn, path,
};
use aethereal_data::{Bound, FilePath, KindName, Plain};
use aethereal_files::{FileRead, Files, FilesListed, ListFiles, Namespace};
use std::collections::{BTreeMap, BTreeSet};

/// What the library is made from: the engine's one publisher, which its chassis hands it as it bootstraps (ADR-0054).
#[derive(Bound)]
#[kind(name = "aether.library.config")]
pub struct LibraryConfig {
    pub publisher: Given<Publisher>,
}

/// The program's code: a singleton root that holds the engine's one publisher and says which blob each kind's head is
/// (ADR-0054). A program lists it after `Files`, with nothing to set. It has no children.
///
/// As it begins it lists `Code`, reads every module there, and publishes the head of every kind each makes. Two
/// modules that make one kind, a module the guests' host refuses, and a listing or a read that fails each stop the
/// program as it starts, logged naming the modules, as a refused tool offer does: the code a program runs is fixed
/// where it is built, so each is a program built wrong. Once `Code` is published it is open: it takes [`Defaults`],
/// for the kinds that have no head, and publishes a module of `Code` again for a [`PublishModule`].
#[derive(Bound)]
#[kind(name = "aether.library")]
pub struct Library {
    publisher: Handle<Publisher>,
    /// Each kind it has published code for: every kind with a head, as it holds the engine's one publisher.
    heads: BTreeSet<KindName>,
    stage: Stage,
    /// The publishes asked of it while it is open, each by the number of the read it asked for it.
    fetching: BTreeMap<u64, Fetching>,
    /// The number of the next read it asks for a publish.
    next: u64,
}

/// Where the library stands as the program starts.
#[derive(Bound)]
enum Stage {
    /// Listing `Code`: the defaults asked meanwhile, in the order asked.
    Listing(Vec<Deferred>),
    /// Reading the modules `Code` lists.
    Reading(Reading),
    /// `Code` is published: defaults are taken, and modules published, as they are asked.
    Open,
    /// `Code` could not be published, and the program stops.
    Stopped,
}

/// The modules of `Code` as the library reads them as the program starts.
#[derive(Bound)]
struct Reading {
    /// The modules read so far.
    read: Vec<Module>,
    /// How many are still being read.
    left: u32,
    /// The defaults asked meanwhile, in the order asked.
    deferred: Vec<Deferred>,
}

/// A module of `Code`, read.
#[derive(Bound)]
struct Module {
    path: FilePath,
    blob: Bytes,
}

/// A publish asked while the library is open: held while the module at `path` in `Code` is read.
#[derive(Bound)]
struct Fetching {
    held: Held<ModulePublished>,
    path: FilePath,
}

/// Defaults asked before `Code` was published: held, and taken once it is.
#[derive(Bound)]
struct Deferred {
    held: Held<Defaulted>,
    modules: Vec<Bytes>,
}

/// What the library keeps for its listing of `Code` as it begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.listing")]
pub struct Listing;

/// What the library keeps for a read of a module of `Code`: what it reads it for.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.reading_module")]
pub enum ReadingModule {
    /// To publish as the program starts: the module's path.
    Starting { path: FilePath },
    /// To publish while it is open, for the publish asked under this number.
    Publishing { number: u64 },
}

/// The library is a root, listed by the program.
impl ChildOf for Library {
    type Parent = Top;
}

/// Asked at its typed path, `path::<Library>(())`, it answers a publish with what became of it.
impl Carries<PublishModule> for Library {
    type Answers = (ModulePublished,);
}

/// Asked at its typed path, it answers defaults with what it took of them.
impl Carries<Defaults> for Library {
    type Answers = (Defaulted,);
}

impl Actor for Library {
    type Config = LibraryConfig;
    type Key = ();
    type Handles = (Answer<FilesListed, Listing>, Answer<FileRead, ReadingModule>, Defaults, PublishModule);
    type BeginSends = (Asks<Path<Files>, ListFiles, Listing>,);
    type EndSends = ();

    fn begin(config: LibraryConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let publisher = turn.take(config.publisher)?;
        turn.to(path::<Files>(())).ask(&ListFiles { namespace: Namespace::Code }, &Listing)?;

        Ok(Self {
            publisher,
            heads: BTreeSet::new(),
            stage: Stage::Listing(Vec::new()),
            fetching: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A module of `Code` read, for what it was read for.
impl Handles<Answer<FileRead, ReadingModule>> for Library {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, reading): (Answered<FileRead>, ReadingModule),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        match reading {
            ReadingModule::Starting { path } => self.read_as_starting(path, answered, turn),
            ReadingModule::Publishing { number } => self.read_to_publish(number, answered, turn),
        }
    }
}

impl Library {
    /// Publishes `blob` as the head of `kind`, and answers with the number the engine gives the publish.
    ///
    /// # Errors
    ///
    /// Why the engine refused it, as `turn.publish` says.
    fn publish<S>(&mut self, kind: &KindName, blob: &Bytes, turn: &mut Turn<'_, Self, S>) -> Result<u64, Refused> {
        let publish = turn.publish(&self.publisher, kind, blob)?;
        self.heads.insert(kind.clone());

        Ok(publish)
    }

    /// Whether code has been published for `kind`.
    fn has_head(&self, kind: &KindName) -> bool {
        self.heads.contains(kind)
    }
}
