//! Passes named by types: the frame's maker and the actors that place things share a type, and a pass is named on the
//! wire by the type's id, as a kind is (ADR-0006, ADR-0025).

use aethereal_data::{Kind, KindId, KindName, Plain};

/// A pass of a frame, named by its type. A name mistyped, or renamed on one side, is a compile error, where a string
/// would join nothing.
///
/// ```
/// use aethereal_data::{Kind, KindName, Plain};
/// use aethereal_render::pass::Pass;
///
/// #[derive(Plain)]
/// struct Surfaces;
///
/// impl Kind for Surfaces {
///     const NAME: KindName = KindName::new("aether.example.surfaces");
/// }
///
/// impl Pass for Surfaces {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a pass",
    label = "a pass is named by a type that implements `Pass`",
    note = "name a pass by a kind of its own: `impl Pass for {Self} {{}}`"
)]
pub trait Pass: Kind {}

/// A pass, by its type's id: what a placement joins and a frame lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct PassName {
    id: KindId,
}

impl Kind for PassName {
    const NAME: KindName = KindName::new("aether.render.pass_name");
}

impl PassName {
    /// The name of the pass `P`.
    #[must_use]
    pub const fn of<P: Pass>() -> Self {
        Self { id: KindId::of::<P>() }
    }
}

/// The compute pass of the renderer's own main frame of each window, before [`Main`]: every dispatch that joins it runs
/// there, so what it writes is drawn in `Main` in the same `Render` (ADR-0026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Compute;

impl Kind for Compute {
    const NAME: KindName = KindName::new("aether.render.compute");
}

impl Pass for Compute {}

/// The pass of the renderer's own main frame of each window that draws: everything drawn onto the window until an
/// actor makes a main frame of its own (ADR-0023, ADR-0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Main;

impl Kind for Main {
    const NAME: KindName = KindName::new("aether.render.main");
}

impl Pass for Main {}
