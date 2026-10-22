//! A program's roots, a list, and what bootstrapping them in order asks: their sections composed, each one's needs
//! found among what those before it provide, and each one begun (ADR-0029).

use super::{Bootstrap, Bootstrapping, Gathers, Host, ProgramError, Provision, Reader};
use crate::Engine;
use aethereal_data::config::{Configuration, Parts, RootSettings};

/// The engine, the host's hosting, and the configuration's sections not yet taken, as a program's roots bootstrap:
/// handed from each root's bootstrap to the next, and to the host once the last has.
pub struct Stage<'host, H: Host> {
    pub(super) engine: Engine,
    pub(super) hosting: H::Hosting<'host>,
    pub(super) configuration: Configuration,
}

impl<'host, H: Host> Stage<'host, H> {
    /// The stage of `engine`, `hosting`, and `configuration`.
    pub(super) const fn new(engine: Engine, hosting: H::Hosting<'host>, configuration: Configuration) -> Self {
        Self { engine, hosting, configuration }
    }

    /// The engine and the hosting, the roots bootstrapped.
    pub(super) fn into_made(self) -> (Engine, H::Hosting<'host>) {
        (self.engine, self.hosting)
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

    /// The sections with `S`'s added under `name`, its defaults `defaults`, read by `reader`, when it brings one.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Repeated`] when a section of that name is composed already, naming what reads each.
    pub(super) fn add<S: RootSettings>(
        mut self,
        name: &'static str,
        reader: Reader,
        defaults: &S,
    ) -> Result<Self, ProgramError> {
        if !S::SECTIONED {
            return Ok(self);
        }

        self.read.push(Read { name, reader });
        match S::compose(self.parts, name, defaults) {
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

    /// Bootstraps each root on `stage`, in the order they were listed, each awaited before the next begins, and the
    /// stage with what they provided.
    ///
    /// # Errors
    ///
    /// The first failure: a section not left to take, or a root's bootstrap, naming the root.
    fn bootstrap(stage: Stage<'_, H>) -> impl Future<Output = Result<(Stage<'_, H>, Self::Provided), ProgramError>>;
}

impl<H: Host> Bootstraps<H, ()> for () {
    type Provided = ();

    async fn bootstrap(stage: Stage<'_, H>) -> Result<(Stage<'_, H>, ()), ProgramError> {
        Ok((stage, ()))
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

    async fn bootstrap(stage: Stage<'_, H>) -> Result<(Stage<'_, H>, Self::Provided), ProgramError> {
        let (mut stage, earlier) = S::bootstrap(stage).await?;
        let settings = A::Settings::take(&mut stage.configuration, A::SECTION)?;
        let needs = earlier.gather();

        let mut cx = Bootstrapping::of(stage);
        let provides =
            A::bootstrap(settings, needs, &mut cx).await.map_err(|why| ProgramError::Root { root: A::NAME, why })?;

        Ok((cx.into_stage(), provides.onto(earlier)))
    }
}
