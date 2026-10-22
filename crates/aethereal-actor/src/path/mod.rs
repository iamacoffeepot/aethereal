//! Typed paths: the actor named by its type, the segments above it filled in from each type's parent (ADR-0022).

mod relative;
mod roots;
#[cfg(test)]
mod tests;

pub use relative::{Child, Parent, child, parent};
pub use roots::{HandlersNeedIn, HasRoot, Here, MadeAfter, NeedsIn, Ordered, Placed, There};

use crate::{ChildOf, Top};
use aethereal_data::{ActorKey, ActorPath, Keyed, KindId, KindName, PathSegment, Then};
use alloc::vec::Vec;
use core::marker::PhantomData;

/// A type whose place names one path: each type above it, up to a root, has one type as its parent. Its keys are its
/// keyed segments' keys, from the root.
///
/// It holds for every type that declares where it stands, unless a type on the way stands
/// [`Anywhere`](crate::Anywhere) or is [`Recursive`](crate::Recursive): neither names one place.
///
/// A send at a path is declared with it, and builds only for a kind the path's type handles:
///
/// ```
/// use aethereal_actor::{ChildOf, Path, Tells, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.shut")]
/// # struct Shut;
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $sends:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = (Poke,);
/// #             type BeginSends = $sends;
/// #             type EndSends = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Handles<Poke> for $name {
/// #             type Sends = ();
/// #
/// #             fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host, "aether.example.host", ());
/// # actor!(Caller, "aether.example.caller", (Tells<Path<Host>, Poke>,));
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Caller {
///     type Parent = Top;
/// }
/// ```
///
/// One of a kind it does not handle does not build:
///
/// ```compile_fail
/// use aethereal_actor::{ChildOf, Path, Tells, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.shut")]
/// # struct Shut;
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $sends:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = (Poke,);
/// #             type BeginSends = $sends;
/// #             type EndSends = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Handles<Poke> for $name {
/// #             type Sends = ();
/// #
/// #             fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host, "aether.example.host", ());
/// # actor!(Caller, "aether.example.caller", (Tells<Path<Host>, Shut>,));
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Caller {
///     type Parent = Top;
/// }
/// ```
///
/// Nor does a path to a type that stands in no one place:
///
/// ```compile_fail
/// use aethereal_actor::{Anywhere, ChildOf, Path, Tells, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.shut")]
/// # struct Shut;
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $sends:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = (Poke,);
/// #             type BeginSends = $sends;
/// #             type EndSends = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, Self, $sends>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Handles<Poke> for $name {
/// #             type Sends = ();
/// #
/// #             fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Timer, "aether.example.timer", ());
/// # actor!(Caller, "aether.example.caller", (Tells<Path<Timer>, Poke>,));
///
/// impl ChildOf for Timer {
///     type Parent = Anywhere;
/// }
///
/// impl ChildOf for Caller {
///     type Parent = Top;
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` stands in no one place, so it has no typed path",
    label = "no one path to `{Self}`",
    note = "a path follows each type's one `Parent` up to a root; a type that is `Anywhere` or `Recursive`, or beneath \
            one, is reached from a reference held"
)]
pub trait PathTo {
    /// The keys of the path's keyed segments, from the root: `()` for none, a key alone for one, a tuple for several.
    type Keys;
    /// How many segments the path has.
    const DEPTH: usize;
    /// The path's segments as a declaration holds them, from its last: what a send at it targets.
    const BENEATH: Beneath;

    /// Puts the path's segments, from the root, after those in `segments`.
    fn segments(keys: Self::Keys, segments: &mut Vec<PathSegment>);
}

impl PathTo for Top {
    type Keys = ();
    const DEPTH: usize = 0;
    const BENEATH: Beneath = Beneath::Top;

    fn segments((): (), _: &mut Vec<PathSegment>) {}
}

impl<T: ChildOf> PathTo for T
where
    T::Parent: PathTo,
    <T::Parent as PathTo>::Keys: Then<T::Key>,
{
    type Keys = <<T::Parent as PathTo>::Keys as Then<T::Key>>::Keys;
    const DEPTH: usize = <T::Parent as PathTo>::DEPTH + 1;
    const BENEATH: Beneath = Beneath::Type(&Segment::<T>::OF);

    fn segments(keys: Self::Keys, segments: &mut Vec<PathSegment>) {
        let (above, key) = <<T::Parent as PathTo>::Keys as Then<T::Key>>::split(keys);
        <T::Parent as PathTo>::segments(above, segments);
        segments.push(PathSegment::new(T::NAME, &key));
    }
}

/// What a path's last segment stands beneath, as a declaration holds it: the top, or the segment of a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beneath {
    Top,
    Type(&'static TypeSegment),
}

/// One segment of a typed path as a declaration holds it: its type's name and kind, whether it is keyed, and what it
/// stands beneath. The keys are the send's, not the declaration's.
#[derive(Debug, PartialEq, Eq)]
pub struct TypeSegment {
    pub name: &'static KindName,
    /// The type's kind: what the actor at the place must be (ADR-0006).
    pub kind: KindId,
    pub keyed: Keyed,
    pub beneath: Beneath,
}

impl TypeSegment {
    /// The path to the place this segment names, when every segment on it is a singleton's, so its names alone
    /// spell it: what a spawn checks is live (ADR-0022). None when one is keyed, which no declaration can name the
    /// key of, or the path would be deeper than any path.
    #[must_use]
    pub fn singletons(&self) -> Option<ActorPath> {
        let mut names = Vec::new();
        let mut segment = self;
        loop {
            let is_too_deep = names.len() >= ActorPath::MAX_DEPTH;
            if segment.keyed == Keyed::Instanced || is_too_deep {
                return None;
            }
            names.push(PathSegment::new(segment.name.clone(), &()));
            match segment.beneath {
                Beneath::Top => break,
                Beneath::Type(above) => segment = above,
            }
        }
        let mut names = names.iter().rev();
        let root = ActorPath::from_root(names.next()?);

        names.try_fold(root, |path, name| path.child(name).ok())
    }
}

/// The segment of `T`, held as a constant, so a declaration can refer to it for as long as the program runs.
struct Segment<T>(PhantomData<T>);

impl<T: ChildOf> Segment<T>
where
    T::Parent: PathTo,
{
    const OF: TypeSegment = TypeSegment {
        name: &T::NAME,
        kind: KindId::of::<T>(),
        keyed: <T::Key as ActorKey>::KEYED,
        beneath: <T::Parent as PathTo>::BENEATH,
    };
}

/// The actor of type `L` at its typed path, under its keys: a place, not an actor that is live (ADR-0022).
///
/// A send at it finds whatever actor of `L`'s type is live there as it is sent, and drops the mail when none is.
#[derive(Debug, PartialEq, Eq)]
pub struct Path<L> {
    path: ActorPath,
    of: PhantomData<fn() -> L>,
}

impl<L> Clone for Path<L> {
    fn clone(&self) -> Self {
        Self { path: self.path.clone(), of: PhantomData }
    }
}

impl<L> Path<L> {
    /// The path, as its names spell it.
    #[must_use]
    pub const fn names(&self) -> &ActorPath {
        &self.path
    }
}

/// The typed path to the actor of type `L` under `keys`, its keyed segments' keys from the root: `()` for a path of
/// singletons, a key alone for one keyed segment, a tuple for several.
///
/// ```text
/// path::<Lens>(())                    // Host, Camera, Lens
/// path::<Banner>(3)                   // the Banner beneath Army 3
/// path::<Squad>((3, Name("scout")))   // World, Army 3, Hud, Squad "scout"
/// ```
///
/// Nothing is looked up when it is made, and the actor's code never handles an id: the engine finds the actor at
/// each send.
#[must_use]
pub fn path<L: ChildOf + PathTo>(keys: L::Keys) -> Path<L> {
    const { assert!(L::DEPTH <= ActorPath::MAX_DEPTH, "a typed path is at most as deep as any path") };
    let mut segments = Vec::with_capacity(L::DEPTH);
    L::segments(keys, &mut segments);
    let mut segments = segments.iter();
    let Some(root) = segments.next() else {
        unreachable!("a type that declares where it stands is a segment of its path");
    };
    let path = segments.fold(ActorPath::from_root(root), |path, segment| {
        let Ok(child) = path.child(segment) else {
            unreachable!("its depth is checked as it builds");
        };

        child
    });

    Path { path, of: PhantomData }
}
