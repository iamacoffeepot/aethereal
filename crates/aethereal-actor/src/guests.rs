//! The one seam through which the engine runs guests' code, whatever runtime its host has (ADR-0001, ADR-0015).

#[cfg(feature = "std")]
use crate::ActorsWriter;
use crate::{
    Above, Actor, Actors, ActorsReader, Asked, Beginning, BlobId, BlobType, Declared, Fault, Instances, Keeping, Keeps,
    Refused, Runs, Seated, Table,
};
use aethereal_data::{ActorPath, Budget, Decode, KindName, Reader};
#[cfg(feature = "std")]
use aethereal_data::{Encode, EncodeError, Writer};
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
    /// blob `code` pins, which `made` is of, and answers with the child as the engine runs it: the child keeps a
    /// clone of `code` while it runs (ADR-0009).
    ///
    /// # Errors
    ///
    /// [`Refused::Began`] when its `begin` failed, and [`Refused::Waiting`] when the instance it would begin in is
    /// waiting on a call out of its own (ADR-0015).
    fn begin(
        &self,
        #[cfg(feature = "std")] code: &crate::BlobRef,
        made: &'static BlobType,
        config: &[u8],
        birth: Birth<'_>,
    ) -> Result<Box<dyn Runs>, Refused>;

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

    fn begin(
        &self,
        #[cfg(feature = "std")] _: &crate::BlobRef,
        _: &'static BlobType,
        _: &[u8],
        _: Birth<'_>,
    ) -> Result<Box<dyn Runs>, Refused> {
        Err(Refused::NotMade)
    }
}

/// What a child begun from a blob is lent as it begins, in the turn that spawns it or as a root between steps: who
/// it is, where it stands and what is above it, the table, what the turn has asked for so far, its part's
/// instances, the pins on the blobs its config refers to, each at where its reference begins, for code that
/// takes pins of its own as it reads them (ADR-0014), where what it keeps is kept, and the tickets of the resources
/// its config holds (ADR-0010).
pub struct Birth<'a> {
    pub me: Seated,
    pub path: &'a ActorPath,
    pub above: Above,
    pub table: &'a dyn Table,
    pub asked: &'a mut Asked,
    pub instances: &'a mut Instances,
    #[cfg(feature = "std")]
    pub refers: &'a [crate::Pinning],
    pub keeping: &'a mut Keeping,
    pub carried: &'a [u64],
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
    let mut budget = Budget::new(birth.instances.read_limit());
    // The config holds the pins its birth refers to, each at where its reference begins in it.
    let pins = crate::PinSource::lying(config, birth.refers);
    let mut reader = Reader::<Actors>::new(config, &mut budget, ActorsReader::of(&pins));
    let config = A::Config::decode(&mut reader).map_err(|error| Refused::Began(Fault::from(error)))?;
    let Birth { me, path, above, table, asked, instances, keeping, .. } = birth;
    let mut beginning = Beginning::beneath(me, path, above, table, asked, instances, Keeps::Here(keeping));
    let actor = A::begin(config, &mut beginning);

    Ok(Box::new(actor.map_err(Refused::Began)?))
}

/// A config for a child begun in other code, as it crosses to that code at [`Actors`]: its bytes, the pins on the
/// blobs it refers to, each at where its reference begins, kept until the child's code has taken its own (ADR-0014),
/// and the tickets of the resources it holds (ADR-0010).
#[cfg(feature = "std")]
pub struct Config {
    pub bytes: Vec<u8>,
    pub held: Vec<crate::Pinning>,
    pub carried: Vec<u64>,
}

/// `config`, a config for a child begun in other code, as it crosses to that code.
///
/// # Errors
///
/// When it does not encode.
#[cfg(feature = "std")]
pub fn config_bytes<C: Encode<Actors>>(config: &C) -> Result<Config, EncodeError> {
    let mut held = Vec::new();
    let mut carried = Vec::new();
    let mut out = Writer::<Actors>::new(ActorsWriter::carrying(&mut held, &mut carried));
    config.encode(&mut out)?;
    let bytes = out.into_bytes();

    Ok(Config { bytes, held, carried })
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
