//! Requests and their answers: what an asker declares, what it is handed back, and how it handles it (ADR-0017).

use crate::declared::{BackTo, Carries, Handled, Handles, Kinds, Listed, Marker, RepliesTo, ReplyWithin, Role};
use crate::declared::{Sending, Target};
use crate::{Actors, Item, Within, Written};
use aethereal_data::{Decode, Kind, KindId, KindName, Plain};
use core::marker::PhantomData;

/// A handler may ask an actor that takes the named protocol `P` a request of kind `K`, keeping a context of kind `C`
/// until it is answered (ADR-0017).
///
/// It builds only when `P` answers `K` with one kind, and the actor handles that answer with `C`:
/// [`Answer<A, C>`](Answer). An asker that does not handle the answer does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, Declared, Ending, Fault, Handles, Protocol, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct WhichTile;
/// #
/// # impl Kind for WhichTile {
/// #     const NAME: KindName = KindName::new("aether.example.which_tile");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct TileIs;
/// #
/// # impl Kind for TileIs {
/// #     const NAME: KindName = KindName::new("aether.example.tile_is");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Looking;
/// #
/// # impl Kind for Looking {
/// #     const NAME: KindName = KindName::new("aether.example.looking");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Glancing;
/// #
/// # impl Kind for Glancing {
/// #     const NAME: KindName = KindName::new("aether.example.glancing");
/// # }
/// #
/// # struct Tiles;
/// #
/// # impl Protocol for Tiles {
/// #     type Kinds = (WhichTile,);
/// # }
/// #
/// # impl Carries<WhichTile> for Tiles {
/// #     type Answers = (TileIs,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Pointer;
/// #
/// # impl Kind for Pointer {
/// #     const NAME: KindName = KindName::new("aether.example.pointer");
/// # }
/// #
/// impl Actor for Pointer {
///     type Config = Pointer;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (Asks<Tiles, WhichTile, Looking>,);
///     type EndSends = ();
///
///     fn begin(pointer: Pointer, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(pointer)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
pub struct Asks<P, K, C>(PhantomData<Of<P, K, C>>);

/// What an [`Asks`] is of: the protocol, the kind asked, and the context kept.
type Of<P, K, C> = fn() -> (P, K, C);

/// What an asker handles: what became of a request it asked whose answer is of kind `A`, with the context of kind `C`
/// it kept (ADR-0017).
///
/// Its handler is handed `(Answered<A>, C)`. An actor handles each answer kind with one kind of context, and one that
/// handles an answer kind with two does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, Declared, Ending, Fault, Handles, Protocol, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct WhichTile;
/// #
/// # impl Kind for WhichTile {
/// #     const NAME: KindName = KindName::new("aether.example.which_tile");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct TileIs;
/// #
/// # impl Kind for TileIs {
/// #     const NAME: KindName = KindName::new("aether.example.tile_is");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Looking;
/// #
/// # impl Kind for Looking {
/// #     const NAME: KindName = KindName::new("aether.example.looking");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Glancing;
/// #
/// # impl Kind for Glancing {
/// #     const NAME: KindName = KindName::new("aether.example.glancing");
/// # }
/// #
/// # struct Tiles;
/// #
/// # impl Protocol for Tiles {
/// #     type Kinds = (WhichTile,);
/// # }
/// #
/// # impl Carries<WhichTile> for Tiles {
/// #     type Answers = (TileIs,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Pointer;
/// #
/// # impl Kind for Pointer {
/// #     const NAME: KindName = KindName::new("aether.example.pointer");
/// # }
/// #
/// impl Actor for Pointer {
///     type Config = Pointer;
///     type Key = ();
///     type Handles = (Answer<TileIs, Looking>, Answer<TileIs, Glancing>);
///     type BeginSends = ();
///     type EndSends = ();
///
///     fn begin(pointer: Pointer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
///         Ok(pointer)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Answer<TileIs, Looking>> for Pointer {
///     type Sends = ();
///
///     fn handle(&mut self, _: (Answered<TileIs>, Looking), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Answer<TileIs, Glancing>> for Pointer {
///     type Sends = ();
///
///     fn handle(&mut self, _: (Answered<TileIs>, Glancing), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// const DECLARED: &Declared = Declared::of::<Pointer>();
/// ```
pub struct Answer<A, C>(PhantomData<fn() -> (A, C)>);

/// What became of a request: its answer, or why none came (ADR-0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered<A> {
    /// The receiver's answer.
    Yes(A),
    /// No answer came, and the engine says why.
    No(Unanswered),
}

/// Why a request was not answered (ADR-0017).
///
/// What the engine says of such a request is mail of this kind, from the actor asked, to the asker, tied to the request
/// as only the engine ties mail ([`Tie::unanswered`](crate::Tie::unanswered)), so no actor can say it in the engine's
/// place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Unanswered {
    /// The receiver had ended, or was gone, when the request was to be handed to it, or it ended owing the answer.
    Ended,
    /// The turn that handled the request failed.
    Failed,
    /// The turn that handled the request neither answered it nor held it, or the receiver held it and let it go
    /// without answering it.
    Unreplied,
}

impl Kind for Unanswered {
    const NAME: KindName = KindName::new("aether.actor.unanswered");
}

/// A request a turn asked, and the context its asker keeps until it is answered, as the engine files it: under the
/// request's count, written at [`Within`] with a pin on each blob it refers to (ADR-0014, ADR-0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asking {
    /// The asker's count of the request, which with the asker is its id.
    pub count: u64,
    /// The kind the request is answered with, whose handler is handed what became of it.
    pub answer: KindId,
    /// The context.
    pub context: Written,
}

impl<P: Listed, K: Kind, C> Marker for Asks<P, K, C> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Handling(<P::Kinds as Kinds>::KINDS));
}

/// An asker handles the one kind its request is answered with, with its context.
impl<T, P, K, A, C> BackTo<T> for Asks<P, K, C>
where
    P: Carries<K, Answers = (A,)>,
    K: Kind,
    A: Kind + Decode<Actors> + 'static,
    C: Kind + Decode<Within> + 'static,
    T: Handles<Answer<A, C>>,
{
}

impl<T, P, K, C> RepliesTo<T> for Asks<P, K, C> {}

impl<Answers, P, K, C> ReplyWithin<Answers, ()> for Asks<P, K, C> {}

impl<A: Kind + Decode<Actors> + 'static, C: Kind + Decode<Within> + 'static> Handled for Answer<A, C> {
    const ROLE: Role = Role::Answer(KindId::of::<A>());
    type Item<'a> = (Answered<A>, C);

    fn take(item: Item<'_>) -> Result<(Answered<A>, C), crate::Fault> {
        let (answered, mail, asking) = match item {
            Item::Answer { mail, asking } => (Answered::Yes(mail.contents()?), mail, asking),
            Item::Unanswered { mail, asking } => (Answered::No(mail.contents()?), mail, asking),
            Item::Mail(_) | Item::Ended(_) => unreachable!("only what became of a request is handed to its handler"),
        };
        let context = asking.context.read(mail.limit())?;

        Ok((answered, context))
    }
}
