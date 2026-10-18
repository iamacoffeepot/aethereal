//! The one seam through which the engine runs guests' code, whatever runtime its host has (ADR-0001, ADR-0015).

use crate::{
    Above, Actor, Actors, ActorsReader, ActorsWriter, Asked, Beginning, BlobId, BlobType, Declared, Fault, Instances,
    Refused, Runs, Seated, Table,
};
use aethereal_data::{ActorPath, Budget, Decode, Encode, EncodeError, KindName, Reader, Writer};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// The guests' code an engine runs: the blobs that are code, and how a child of one begins (ADR-0015).
///
/// The engine holds one, set as it is made, and each part's [`Instances`]
/// carries it, so a turn that spawns from a blob reaches it through what it
/// is lent. Nothing outside an implementation names a runtime's types.
pub trait Guests: Send + Sync {
    /// Takes in the blob `code`, whose bytes are `bytes`, as code its actors can be made from: refused when it is
    /// not fit to run, and then nothing is made from it.
    ///
    /// # Errors
    ///
    /// Why it is not fit.
    fn load(&self, code: BlobId, bytes: &[u8]) -> Result<(), Unfit>;

    /// Lets go of the blob `code`, which the engine no longer holds. Nothing when it is not code.
    fn unload(&self, code: BlobId);

    /// The actor type the blob `code` makes that declares itself as `declared` does: nothing when the blob is not
    /// code, or makes no such type.
    fn made(&self, code: BlobId, declared: &'static Declared) -> Option<&'static BlobType>;

    /// Begins `birth`'s child, of the type `made`, from its config's bytes `config`, in its part's instance of the
    /// blob, and answers with the child as the engine runs it.
    ///
    /// # Errors
    ///
    /// [`Refused::Began`] when its `begin` failed, and [`Refused::Waiting`] when the instance it would begin in is
    /// waiting on a call out of its own (ADR-0015).
    fn begin(&self, made: &'static BlobType, config: &[u8], birth: Birth<'_>) -> Result<Box<dyn Runs>, Refused>;

    /// Begins `birth`'s child, of the native type `declared`, which the code that spawns it does not hold, from its
    /// config's bytes `config`: for a guest, whose host begins it from the types its program keeps by name
    /// (ADR-0009, ADR-0015). Code that holds every type it spawns, as a native program does, never asks, and is
    /// refused.
    ///
    /// # Errors
    ///
    /// As [`Self::begin`], and [`Refused::NotMade`] when the program keeps no such type.
    fn begin_linked(&self, _: &'static Declared, _: &[u8], _: Birth<'_>) -> Result<Box<dyn Runs>, Refused> {
        Err(Refused::NotMade)
    }
}

/// The guests of an engine that runs none: it takes in no code, so nothing is made from a blob.
pub struct NoGuests;

impl Guests for NoGuests {
    fn load(&self, _: BlobId, _: &[u8]) -> Result<(), Unfit> {
        Err(Unfit::NoRuntime)
    }

    fn unload(&self, _: BlobId) {}

    fn made(&self, _: BlobId, _: &'static Declared) -> Option<&'static BlobType> {
        None
    }

    fn begin(&self, _: &'static BlobType, _: &[u8], _: Birth<'_>) -> Result<Box<dyn Runs>, Refused> {
        Err(Refused::NotMade)
    }
}

/// What a child begun from a blob is lent as it begins, in the turn that spawns it or as a root between steps: who
/// it is, where it stands and what is above it, the table, what the turn has asked for so far, and its part's
/// instances.
pub struct Birth<'a> {
    pub me: Seated,
    pub path: &'a ActorPath,
    pub above: Above,
    pub table: &'a dyn Table,
    pub asked: &'a mut Asked,
    pub instances: &'a mut Instances,
}

/// The native actor types a program keeps by name, which a guest spawns by naming its kind, and its host begins
/// (ADR-0009). Nothing else reads them: a native turn begins every native type it spawns itself.
#[derive(Default)]
pub struct Links {
    linked: Vec<Link>,
}

/// One native type a program keeps: what it declares, and what begins one from its config's bytes.
#[derive(Clone, Copy)]
pub struct Link {
    pub declared: &'static Declared,
    pub begin: BeginLinked,
}

/// Begins a child of one native type as a [`Birth`] says, from its config's bytes.
pub type BeginLinked = fn(&[u8], Birth<'_>) -> Result<Box<dyn Runs>, Refused>;

/// The most bytes a config read for a native type a guest spawned allocates, for now: the engine's own limit is to be
/// handed here.
const CONFIG_LIMIT: usize = 1 << 20;

impl Links {
    /// These, and the type `A`.
    #[must_use]
    pub fn with<A: Actor>(mut self) -> Self {
        self.linked.push(Link { declared: Declared::of::<A>(), begin: begin_linked::<A> });

        self
    }

    /// The type of the kind `kind`: nothing when the program keeps none of it.
    #[must_use]
    pub fn get(&self, kind: &KindName) -> Option<Link> {
        self.linked.iter().find(|link| link.declared.name() == kind).copied()
    }
}

/// Begins `birth`'s child, of type `A`, from its config's bytes `config`.
fn begin_linked<A: Actor>(config: &[u8], birth: Birth<'_>) -> Result<Box<dyn Runs>, Refused> {
    let mut budget = Budget::new(CONFIG_LIMIT);
    let mut reader = Reader::<Actors>::new(config, &mut budget, ActorsReader::new());
    let config = A::Config::decode(&mut reader).map_err(|error| Refused::Began(Fault::from(error)))?;
    let Birth { me, path, above, table, asked, instances } = birth;
    let actor = A::begin(config, &mut Beginning::beneath::<A>(me, path, above, table, asked, instances));

    Ok(Box::new(actor.map_err(Refused::Began)?))
}

/// The bytes of `config`, a config for a child begun from a blob, as they cross to its code at [`Actors`]. The pins
/// of what it refers to go when they are written: the child's code takes its own as it reads them (ADR-0014).
///
/// # Errors
///
/// When it does not encode.
#[cfg(feature = "std")]
pub fn config_bytes<C: Encode<Actors>>(config: &C) -> Result<Vec<u8>, EncodeError> {
    let mut held = Vec::new();
    let mut out = Writer::<Actors>::new(ActorsWriter::new(&mut held));
    config.encode(&mut out)?;

    Ok(out.into_bytes())
}

/// Why a blob was not taken in as code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unfit {
    /// The engine runs no guests.
    NoRuntime,
    /// The runtime could not compile it.
    Uncompiled(String),
    /// It has no section that declares what it makes, or one that does not read back.
    Undeclared,
    /// It lacks the export of this name.
    Unexported(&'static str),
    /// It imports what the host does not give.
    Unlinked(String),
}
