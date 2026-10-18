//! The one seam through which the engine runs guests' code, whatever runtime its host has (ADR-0001, ADR-0015).

use crate::{Above, Actors, ActorsWriter, Asked, BlobId, BlobType, Instances, Refused, Runs, Seated, Table};
use aethereal_data::{ActorPath, Encode, EncodeError, KindName, Writer};
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

    /// The actor type of kind `kind` the blob `code` makes: nothing when the blob is not code, or makes no such kind.
    fn made(&self, code: BlobId, kind: &KindName) -> Option<&'static BlobType>;

    /// Begins `birth`'s child, of the type `made`, from its config's bytes `config`, in its part's instance of the
    /// blob, and answers with the child as the engine runs it.
    ///
    /// # Errors
    ///
    /// [`Refused::Began`] when its `begin` failed, and [`Refused::Waiting`] when the instance it would begin in is
    /// waiting on a call out of its own (ADR-0015).
    fn begin(&self, made: &'static BlobType, config: &[u8], birth: Birth<'_>) -> Result<Box<dyn Runs>, Refused>;
}

/// The guests of an engine that runs none: it takes in no code, so nothing is made from a blob.
pub struct NoGuests;

impl Guests for NoGuests {
    fn load(&self, _: BlobId, _: &[u8]) -> Result<(), Unfit> {
        Err(Unfit::NoRuntime)
    }

    fn unload(&self, _: BlobId) {}

    fn made(&self, _: BlobId, _: &KindName) -> Option<&'static BlobType> {
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
