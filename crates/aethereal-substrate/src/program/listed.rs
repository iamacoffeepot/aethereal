//! A program's roots, a list, and what bootstrapping them in order asks: their sections composed, each one's needs
//! found among what those before it provide, and each one begun (ADR-0029).

use super::{Bootstrap, Bootstrapping, Gathers, Host, ProgramError, Provision, Reader};
use crate::Engine;
use aethereal_data::config::{Configuration, Parts, RootSettings};

/// The engine, the host's hosting, and the configuration's sections not yet taken, as a program's roots bootstrap.
pub struct Stage<'stage, H: Host> {
    engine: &'stage mut Engine,
    hosting: &'stage mut H::Hosting,
    configuration: Configuration,
}

impl<'stage, H: Host> Stage<'stage, H> {
    /// The stage of `engine`, `hosting`, and `configuration`.
    pub(super) const fn new(
        engine: &'stage mut Engine,
        hosting: &'stage mut H::Hosting,
        configuration: Configuration,
    ) -> Self {
        Self { engine, hosting, configuration }
    }

    /// The engine and the hosting, borrowed together for one root's bootstrap.
    pub(super) const fn engine_and_hosting(&mut self) -> (&mut Engine, &mut H::Hosting) {
        (&mut *self.engine, &mut *self.hosting)
    }
}

/// One section composed so far, and what reads it.
struct Read {
    name: &'static str,
    reader: Reader,
}

/// The sections composed so far, and what reads each, as a program is put together.
pub struct Composing {
    parts: Parts,
    read: Vec<Read>,
}

impl Composing {
    /// No sections yet.
    pub(super) const fn new() -> Self {
        Self { parts: Parts::new(), read: Vec::new() }
    }

    /// The sections with `S`'s added under `name`, read by `reader`, when it brings one.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] when a section of that name is composed already, naming what reads each.
    pub(super) fn add<S: RootSettings>(mut self, name: &'static str, reader: Reader) -> Result<Self, ProgramError> {
        if !S::SECTIONED {
            return Ok(self);
        }

        self.read.push(Read { name, reader });
        match S::compose(self.parts, name) {
            Ok(parts) => Ok(Self { parts, read: self.read }),
            Err(repeated) => {
                let readers = self.read.into_iter().filter(|read| read.name == repeated.name).map(|read| read.reader);
                Err(ProgramError::Repeated { name: repeated.name, readers: readers.collect() })
            }
        }
    }

    /// The sections composed.
    pub(super) fn into_parts(self) -> Parts {
        self.parts
    }
}

/// A program's roots, `()` for none and `(A, S)` for the root `A` listed after those of `S`, bootstrapped in order:
/// each one's needs found among what those before it provided, at the places `I` (ADR-0029).
#[diagnostic::on_unimplemented(
    message = "the roots `{Self}` do not bootstrap in this order on `{H}`",
    label = "a root here needs what no root listed before it provides",
    note = "list the root that provides each type a root needs before it"
)]
pub trait Bootstraps<H: Host, I> {
    /// What the roots provided, the last root's first.
    type Provided;

    /// `composing` with each root's section added, in the order they were listed.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] when two sections share a name.
    fn compose(composing: Composing) -> Result<Composing, ProgramError>;

    /// Bootstraps each root on `stage`, in the order they were listed, and what they provided.
    ///
    /// # Errors
    ///
    /// The first failure: a section not left to take, or a root's bootstrap, naming the root.
    fn bootstrap(stage: &mut Stage<'_, H>) -> Result<Self::Provided, ProgramError>;
}

impl<H: Host> Bootstraps<H, ()> for () {
    type Provided = ();

    fn compose(composing: Composing) -> Result<Composing, ProgramError> {
        Ok(composing)
    }

    fn bootstrap(_: &mut Stage<'_, H>) -> Result<(), ProgramError> {
        Ok(())
    }
}

impl<H: Host, A, S, IA, IS> Bootstraps<H, (IA, IS)> for (A, S)
where
    A: Bootstrap<H>,
    S: Bootstraps<H, IS>,
    S::Provided: Gathers<A::Needs, IA>,
    A::Provides: Provision<S::Provided>,
{
    type Provided = <A::Provides as Provision<S::Provided>>::Onto;

    fn compose(composing: Composing) -> Result<Composing, ProgramError> {
        S::compose(composing)?.add::<A::Settings>(A::SECTION, Reader::Root(A::NAME))
    }

    fn bootstrap(stage: &mut Stage<'_, H>) -> Result<Self::Provided, ProgramError> {
        let earlier = S::bootstrap(stage)?;
        let settings = A::Settings::take(&mut stage.configuration, A::SECTION)?;
        let needs = earlier.gather();

        let provides = A::bootstrap(settings, needs, &mut Bootstrapping::of(stage))
            .map_err(|why| ProgramError::Root { root: A::NAME, why })?;

        Ok(provides.onto(earlier))
    }
}
