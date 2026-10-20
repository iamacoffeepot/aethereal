//! Requests and their answers: what an asker declares, what it is handed back, and how it handles it (ADR-0017).

#[cfg(feature = "std")]
use crate::BlobRef;
use crate::declared::{Carries, DeclaredBy, Handled, Handles, Kinds, Last, Listed, Marker, Protocol, RepliesTo};
use crate::declared::{RepliesWith, ReplyWithin, Role, Sending, Target};
use crate::{Actors, ChildOf, Item, Path, PathTo, ProtocolRef, Seated, Table, Within, Written};
use aethereal_data::{Decode, Kind, KindId, KindName, Plain};
#[cfg(feature = "std")]
use alloc::vec::Vec;
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
    /// The job returned a fault, which is in the asker's log, or its host lost it to a panic (ADR-0018).
    Faulted,
}

impl Kind for Unanswered {
    const NAME: KindName = KindName::new("aether.actor.unanswered");
}

/// A request a turn asked, or a job it ran, and the context its asker keeps until it is answered, as the engine files
/// it: under the request's count, written at [`Within`] with a pin on each blob it refers to (ADR-0014, ADR-0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asking {
    /// The asker's count of the request, which with the asker is its id.
    pub count: u64,
    /// The kind the request is answered with, whose handler is handed what became of it.
    pub answer: KindId,
    /// The context.
    pub context: Written,
    /// A pin on each blob a job refers to, so each is kept until the job is answered or cancelled, or its actor ends,
    /// whenever the job runs: none for a request, whose mail holds its own (ADR-0018).
    #[cfg(feature = "std")]
    pub lent: Vec<BlobRef>,
}

/// What a turn asks a request of kind `K` of, keeping a context of kind `C`: a reference to a named protocol, or a
/// typed path, whose type says what it answers (ADR-0017, ADR-0022).
///
/// A type asked at a typed path says what it answers each kind it can be asked with [`Carries`], beside its handler:
///
/// ```
/// # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path};
/// # use aethereal_actor::{ToSender, Top, Turn, path};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! kind {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #     };
/// # }
/// # kind!(Hello, "aether.example.hello");
/// # kind!(Hi, "aether.example.hi");
/// # kind!(Bye, "aether.example.bye");
/// # kind!(Waiting, "aether.example.waiting");
/// # kind!(Greeter, "aether.example.greeter");
/// # kind!(Visitor, "aether.example.visitor");
/// #
/// # impl ChildOf for Greeter {
/// #     type Parent = Top;
/// # }
/// #
/// # impl ChildOf for Visitor {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Greeter {
/// #     type Config = Greeter;
/// #     type Key = ();
/// #     type Handles = (Hello,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(greeter: Greeter, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Actor for Visitor {
/// #     type Config = Visitor;
/// #     type Key = ();
/// #     type Handles = (Answer<Hi, Waiting>,);
/// #     type BeginSends = (Asks<Path<Greeter>, Hello, Waiting>,);
/// #     type EndSends = ();
/// #
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
/// #         turn.ask_to(path::<Greeter>(()), &Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Answer<Hi, Waiting>> for Visitor {
/// #     type Sends = ();
/// #
/// #     fn handle(&mut self, _: (Answered<Hi>, Waiting), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// impl Carries<Hello> for Greeter {
///     type Answers = (Hi,);
/// }
///
/// impl Handles<Hello> for Greeter {
///     type Sends = (ToSender<Hi>,);
///
///     fn handle(&mut self, _: Hello, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Hi)?)
///     }
/// }
/// ```
///
/// A type that does not say what it answers the kind is not asked it at a typed path:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path};
/// # use aethereal_actor::{ToSender, Top, Turn, path};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! kind {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #     };
/// # }
/// # kind!(Hello, "aether.example.hello");
/// # kind!(Hi, "aether.example.hi");
/// # kind!(Bye, "aether.example.bye");
/// # kind!(Waiting, "aether.example.waiting");
/// # kind!(Greeter, "aether.example.greeter");
/// # kind!(Visitor, "aether.example.visitor");
/// #
/// # impl ChildOf for Greeter {
/// #     type Parent = Top;
/// # }
/// #
/// # impl ChildOf for Visitor {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Greeter {
/// #     type Config = Greeter;
/// #     type Key = ();
/// #     type Handles = (Hello,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(greeter: Greeter, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Actor for Visitor {
/// #     type Config = Visitor;
/// #     type Key = ();
/// #     type Handles = (Answer<Hi, Waiting>,);
/// #     type BeginSends = (Asks<Path<Greeter>, Hello, Waiting>,);
/// #     type EndSends = ();
/// #
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
/// #         turn.ask_to(path::<Greeter>(()), &Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Answer<Hi, Waiting>> for Visitor {
/// #     type Sends = ();
/// #
/// #     fn handle(&mut self, _: (Answered<Hi>, Waiting), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// // No `Carries<Hello>` for `Greeter`.
///
/// impl Handles<Hello> for Greeter {
///     type Sends = (ToSender<Hi>,);
///
///     fn handle(&mut self, _: Hello, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Hi)?)
///     }
/// }
/// ```
///
/// Nor is one whose handler replies with a kind it does not say it answers with:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path};
/// # use aethereal_actor::{ToSender, Top, Turn, path};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! kind {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #     };
/// # }
/// # kind!(Hello, "aether.example.hello");
/// # kind!(Hi, "aether.example.hi");
/// # kind!(Bye, "aether.example.bye");
/// # kind!(Waiting, "aether.example.waiting");
/// # kind!(Greeter, "aether.example.greeter");
/// # kind!(Visitor, "aether.example.visitor");
/// #
/// # impl ChildOf for Greeter {
/// #     type Parent = Top;
/// # }
/// #
/// # impl ChildOf for Visitor {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Greeter {
/// #     type Config = Greeter;
/// #     type Key = ();
/// #     type Handles = (Hello,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(greeter: Greeter, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Actor for Visitor {
/// #     type Config = Visitor;
/// #     type Key = ();
/// #     type Handles = (Answer<Hi, Waiting>,);
/// #     type BeginSends = (Asks<Path<Greeter>, Hello, Waiting>,);
/// #     type EndSends = ();
/// #
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
/// #         turn.ask_to(path::<Greeter>(()), &Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Answer<Hi, Waiting>> for Visitor {
/// #     type Sends = ();
/// #
/// #     fn handle(&mut self, _: (Answered<Hi>, Waiting), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// impl Carries<Hello> for Greeter {
///     type Answers = (Hi,);
/// }
///
/// impl Handles<Hello> for Greeter {
///     type Sends = (ToSender<Bye>,);
///
///     fn handle(&mut self, _: Hello, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Bye)?)
///     }
/// }
/// ```
pub trait Askable<K, C> {
    /// What a turn's sends list for it: [`Asks<P, K, C>`] for a [`ProtocolRef<P>`], and `Asks<Path<L>, K, C>` for a
    /// [`Path<L>`].
    type Marker;

    /// The one kind the request is answered with.
    type Answer: Kind;

    /// The actor asked as the request is sent, and its seat, read from `table` when it is a path: the place in
    /// [`SeatNumber::NOWHERE`](crate::SeatNumber::NOWHERE) when it holds no live actor of its type, where the request
    /// is answered [`Unanswered::Ended`].
    fn asked(self, table: &dyn Table) -> Seated;
}

impl<P: Protocol + Carries<K, Answers = (A,)>, K: Kind, A: Kind, C> Askable<K, C> for ProtocolRef<P> {
    type Marker = Asks<P, K, C>;
    type Answer = A;

    fn asked(self, _: &dyn Table) -> Seated {
        self.seated()
    }
}

/// A type asked at a typed path says what it answers each kind it can be asked with its own [`Carries`], beside its
/// handler, as a named protocol does (ADR-0022).
impl<L, K, A, C> Askable<K, C> for Path<L>
where
    L: ChildOf + PathTo + Handles<K> + Carries<K, Answers = (A,)>,
    K: Kind + Handled,
    A: Kind,
{
    type Marker = Asks<Self, K, C>;
    type Answer = A;

    fn asked(self, table: &dyn Table) -> Seated {
        table.reach(self.names(), KindId::of::<L>())
    }
}

impl<P: Listed, K: Kind, C> Marker for Asks<P, K, C> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Handling(<P::Kinds as Kinds>::KINDS));
}

impl<L: ChildOf + PathTo + Handles<K>, K: Kind + Handled, C> Marker for Asks<Path<L>, K, C> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Path(Last::<L>::SEGMENT));
}

/// An asker handles the one kind its request is answered with, with its context.
impl<T, P, K, A, C> DeclaredBy<T> for Asks<P, K, C>
where
    P: Protocol + Carries<K, Answers = (A,)>,
    K: Kind,
    A: Kind + Decode<Actors> + 'static,
    C: Kind + Decode<Within> + 'static,
    T: Handles<Answer<A, C>>,
{
}

/// An asker at a typed path handles the one kind the path's type answers with, with its context, and that type's
/// handler replies with that kind alone: checked here, where the ask is declared, and not at the call (ADR-0022).
impl<T, L, K, A, C> DeclaredBy<T> for Asks<Path<L>, K, C>
where
    L: ChildOf + PathTo + Handles<K> + Carries<K, Answers = (A,)>,
    K: Kind + Handled,
    A: Kind + Decode<Actors> + 'static,
    C: Kind + Decode<Within> + 'static,
    T: Handles<Answer<A, C>>,
    <L as Handles<K>>::Sends: RepliesWith<A>,
{
}

impl<T, P, K, C> RepliesTo<T> for Asks<P, K, C> {}

impl<Answers, P, K, C> ReplyWithin<Answers, ()> for Asks<P, K, C> {}

impl<A, P, K, C> RepliesWith<A> for Asks<P, K, C> {}

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
