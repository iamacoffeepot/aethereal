//! Requests and their answers: what an asker declares, what it is handed back, and how it handles it (ADR-0017).

#[cfg(feature = "std")]
use crate::Bytes;
use crate::declared::{Carries, DependsOn, Handled, Handles, Lists, Protocol, Role, Untaken};
use crate::{ActorRef, Actors, Admits, ChildOf, Due, Item, Mail, Path, PathTo, ProtocolRef, Receives, Within, Written};
use aethereal_data::{Decode, DecodeError, Kind, KindId, Plain};
#[cfg(feature = "std")]
use alloc::vec::Vec;
use core::marker::PhantomData;

/// What an asker handles: what became of a request it asked whose answer is of kind `A`, with the context of kind `C`
/// it kept (ADR-0017).
///
/// Its handler is handed `(Answered<A>, C)`. An actor handles each answer kind with one kind of context, and one that
/// handles an answer kind with two does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Answer, Answered, Beginning, Carries, Declared, Ending, Fault, Protocol, Receives, Turn};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.which_tile")]
/// # struct WhichTile;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.tile_is")]
/// # struct TileIs;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.looking")]
/// # struct Looking;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.glancing")]
/// # struct Glancing;
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
/// # #[kind(name = "aether.example.pointer")]
/// # struct Pointer;
/// #
/// # impl aethereal_actor::ChildOf for Pointer {
/// #     type Parent = aethereal_actor::Top;
/// # }
/// #
/// impl Actor for Pointer {
///     type Config = Pointer;
///     type Key = ();
///     type Handles = (Answer<TileIs, Looking>, Answer<TileIs, Glancing>);
///     type Depends = ();
///     type Spawns = ();
///
///     fn begin(pointer: Pointer, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         Ok(pointer)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Receives<Answer<TileIs, Looking>> for Pointer {
///     fn handle(&mut self, _: (Answered<TileIs>, Looking), _: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Receives<Answer<TileIs, Glancing>> for Pointer {
///     fn handle(&mut self, _: (Answered<TileIs>, Glancing), _: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
#[kind(name = "aether.actor.unanswered")]
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
    /// The request's deadline came before its answer, and the engine stopped waiting for it: an answer that comes
    /// later is dropped (ADR-0041).
    Expired,
    /// The receiver could not read the request within the engine's `read_limit`, or its bytes did not decode as its
    /// kind, so its handler did not run; or the answer came and the asker could not read it, which is its sender's
    /// fault and not the asker's (ADR-0017).
    Unreadable,
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
    pub lent: Vec<Bytes>,
    /// When the engine stops waiting for its answer, and answers it [`Unanswered::Expired`]: `Never` for a job, which
    /// its asker cancels (ADR-0018, ADR-0041).
    pub due: Due,
}

/// What a turn asks a request of kind `K` of, keeping a context of kind `C`: a reference to a named protocol, or a
/// typed path or a reference to a type, whose type says what it answers (ADR-0017, ADR-0022).
///
/// A type asked at a typed path answers each kind it can be asked with what its handler returns, its [`Carries`]:
///
/// ```
/// # use aethereal_actor::{Answered, Beginning, Fault};
/// # use aethereal_actor::{actor, path};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hello")]
/// # struct Hello;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hi")]
/// # struct Hi;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.bye")]
/// # struct Bye;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.waiting")]
/// # struct Waiting;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.greeter")]
/// # struct Greeter;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.visitor")]
/// # struct Visitor;
/// #
/// #[actor]
/// impl Greeter {
/// #     fn begin(greeter: Greeter) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
///     #[request]
///     fn hello(&mut self, _: Hello) -> Result<Hi, Fault> {
///         Ok(Hi)
///     }
/// }
/// #
/// # #[actor(depends(Greeter))]
/// # impl Visitor {
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #         turn.to(path::<Greeter>(())).ask(&Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     #[answer]
/// #     fn hi(&mut self, _: (Answered<Hi>, Waiting)) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
///
/// A type whose handler of the kind answers nothing is not asked it at a typed path:
///
/// ```compile_fail
/// # use aethereal_actor::{Answered, Beginning, Fault};
/// # use aethereal_actor::{actor, path};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hello")]
/// # struct Hello;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hi")]
/// # struct Hi;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.bye")]
/// # struct Bye;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.waiting")]
/// # struct Waiting;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.greeter")]
/// # struct Greeter;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.visitor")]
/// # struct Visitor;
/// #
/// #[actor]
/// impl Greeter {
/// #     fn begin(greeter: Greeter) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
///     // Answers `Hello` with nothing: it is a tell to `Greeter`.
///     #[tell]
///     fn hello(&mut self, _: Hello) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// #
/// # #[actor(depends(Greeter))]
/// # impl Visitor {
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #         turn.to(path::<Greeter>(())).ask(&Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     #[answer]
/// #     fn hi(&mut self, _: (Answered<Hi>, Waiting)) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
///
/// An actor is asked through a reference to it by the same rule as at its typed path: its handler returns what it
/// answers, its [`Carries`], and the asker handles that answer with its context:
///
/// ```
/// # use aethereal_actor::{Answered, Beginning, Fault};
/// # use aethereal_actor::actor;
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hello")]
/// # struct Hello;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hi")]
/// # struct Hi;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.waiting")]
/// # struct Waiting;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.greeter")]
/// # struct Greeter;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.visitor")]
/// # struct Visitor;
/// #
/// # #[actor(parent = Visitor)]
/// # impl Greeter {
/// #     fn begin(greeter: Greeter) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
/// #     #[request]
/// #     fn hello(&mut self, _: Hello) -> Result<Hi, Fault> {
/// #         Ok(Hi)
/// #     }
/// # }
/// #
/// #[actor(depends(Greeter), spawns(Greeter))]
/// impl Visitor {
///     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         let Ok(greeter) = turn.spawn::<Greeter>(&(), Greeter) else {
///             return Ok(visitor);
///         };
///         turn.to(greeter).ask(&Hello, &Waiting)?;
///
///         Ok(visitor)
///     }
/// #
/// #     #[answer]
/// #     fn hi(&mut self, _: (Answered<Hi>, Waiting)) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// }
/// ```
///
/// A reference to a type whose handler of the kind answers nothing is not asked it:
///
/// ```compile_fail
/// # use aethereal_actor::{Answered, Beginning, Fault};
/// # use aethereal_actor::actor;
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hello")]
/// # struct Hello;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.hi")]
/// # struct Hi;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.waiting")]
/// # struct Waiting;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.greeter")]
/// # struct Greeter;
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.visitor")]
/// # struct Visitor;
/// #
/// #[actor(parent = Visitor)]
/// impl Greeter {
/// #     fn begin(greeter: Greeter) -> Result<Self, Fault> {
/// #         Ok(greeter)
/// #     }
/// #
///     // Answers `Hello` with nothing: it is a tell to `Greeter`.
///     #[tell]
///     fn hello(&mut self, _: Hello) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// #
/// # #[actor(depends(Greeter), spawns(Greeter))]
/// # impl Visitor {
/// #     fn begin(visitor: Visitor, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #         let Ok(greeter) = turn.spawn::<Greeter>(&(), Greeter) else {
/// #             return Ok(visitor);
/// #         };
/// #         turn.to(greeter).ask(&Hello, &Waiting)?;
/// #
/// #         Ok(visitor)
/// #     }
/// #
/// #     #[answer]
/// #     fn hi(&mut self, _: (Answered<Hi>, Waiting)) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
///
/// An asker that does not handle the answer, with the context it keeps, does not build, whether it asks through a
/// reference to a protocol, at a typed path, or through a reference to a type:
///
/// ```compile_fail
/// # use aethereal_actor::{Fault, ProtocolRef, Turn, actor, protocol};
/// # use aethereal_data::{EncodeError, Plain};
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.which_tile")]
/// # struct WhichTile;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.tile_is")]
/// # struct TileIs;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.looking")]
/// # struct Looking;
/// #
/// # #[protocol(WhichTile(answers = TileIs))]
/// # struct Tiles;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.pointer")]
/// # struct Pointer;
/// #
/// /// Handles no answer.
/// #[actor]
/// impl Pointer {
///     fn begin(pointer: Pointer) -> Result<Self, Fault> {
///         Ok(pointer)
///     }
/// }
///
/// fn look(turn: &mut Turn<'_, Pointer>, tiles: ProtocolRef<Tiles>) -> Result<(), EncodeError> {
///     turn.to(tiles).ask(&WhichTile, &Looking)?;
///
///     Ok(())
/// }
/// ```
pub trait Askable<Me, K, C, I> {
    /// The one kind the request is answered with.
    type Answer: Kind;
}

/// An actor of this type is asked mail of kind `K` by its type, at a typed path or through a reference, and answers it
/// with one kind, [`Answer`](Self::Answer): the one it says it answers `K` with in its own [`Carries`], beside its
/// handler, as a named protocol does (ADR-0022).
pub trait AnswersByType<K: Kind>: Handles<K> {
    /// The one kind it answers `K` with.
    type Answer: Kind;
}

impl<L, K, A> AnswersByType<K> for L
where
    L: Handles<K> + Carries<K, Answers = (A,)>,
    K: Kind,
    A: Kind,
{
    type Answer = A;
}

/// An actor of type `T` may declare that it asks an actor of this type, by its type, mail of kind `K`, keeping a
/// context of kind `C`: `T` handles the one kind this type answers `K` with, with `C`, and this type's handler of `K`
/// replies with that kind alone. Checked where the ask is declared, and not at the call, for an ask at a typed path
/// and one through a reference alike (ADR-0022).
pub trait AskedByType<T, K: Kind, C>: AnswersByType<K> {}

impl<T, L, K, A, C> AskedByType<T, K, C> for L
where
    L: AnswersByType<K, Answer = A>,
    K: Kind,
    A: Kind + Decode<Actors> + 'static,
    C: Kind + Decode<Within> + 'static,
    T: Receives<Answer<A, C>>,
{
}

/// A protocol answers a request of one of its kinds with one kind, which the asker handles with its context.
impl<Me, P, K, A, C, I> Askable<Me, K, C, I> for ProtocolRef<P>
where
    P: Protocol + Carries<K, Answers = (A,)>,
    P::Kinds: Lists<K, I>,
    K: Kind,
    A: Kind + Decode<Actors> + 'static,
    C: Kind + Decode<Within> + 'static,
    Me: Receives<Answer<A, C>>,
{
    type Answer = A;
}

/// A type at a typed path is asked as through a reference to it, at a path beneath a root of a type the asker depends
/// on, at `I`, whose guard admits the asker's type, at `J` (ADR-0059).
impl<Me, L, K, C, I, J> Askable<Me, K, C, (I, J)> for Path<L>
where
    Me: DependsOn<L, I>,
    L: ChildOf + PathTo + AskedByType<Me, K, C>,
    L::From: Admits<L, Me, J>,
    K: Kind + Handled,
{
    type Answer = <L as AnswersByType<K>>::Answer;
}

/// A reference to a type is proven to reach it, so it is asked as the type says it answers, by an asker whose type its
/// guard admits (ADR-0059).
impl<Me, L, K, C, I> Askable<Me, K, C, I> for ActorRef<L>
where
    L: AskedByType<Me, K, C>,
    L::From: Admits<L, Me, I>,
    K: Kind + Handled,
{
    type Answer = <L as AnswersByType<K>>::Answer;
}

impl<A: Kind + Decode<Actors> + 'static, C: Kind + Decode<Within> + 'static> Handled for Answer<A, C> {
    const ROLE: Role = Role::Answer(KindId::of::<A>());
    type Item<'a> = (Answered<A>, C);

    /// An answer that does not read is handed over as no answer, [`Unanswered::Unreadable`], with its context: its
    /// sender's fault. So is what the engine says of a request that was not answered, which the engine writes and
    /// always reads, so its handler always runs. A context that does not read fails the turn: the asker wrote it
    /// (ADR-0017).
    fn take(item: Item<'_>) -> Result<(Answered<A>, C), Untaken> {
        let (answered, mail, asking) = match item {
            Item::Answer { mail, asking } => (answered(&mail), mail, asking),
            Item::Unanswered { mail, asking } => (unanswered(mail.contents()), mail, asking),
            Item::Mail(_) | Item::Ended(_) => unreachable!("only what became of a request is handed to its handler"),
        };
        let context = asking.context.read(mail.limit()).map_err(Untaken::failed)?;

        Ok((answered, context))
    }
}

/// What became of a request answered with `mail`: the answer, or none when it is not of the kind asked or did not
/// read (ADR-0017).
fn answered<A: Kind + Decode<Actors> + 'static>(mail: &Mail<'_>) -> Answered<A> {
    // An answer of another kind than the one asked is read as unreadable, as an answer that does not decode is. A
    // native handler's answer is its type's, which the build checks (ADR-0059); this one came from a guest's code, or
    // through a protocol cast to at run time, which checks the kinds it lists and not their answers.
    if mail.kind() != KindId::of::<A>() {
        return Answered::No(Unanswered::Unreadable);
    }

    mail.contents().map_or(Answered::No(Unanswered::Unreadable), Answered::Yes)
}

/// What became of a request the engine says was not answered, as `read` says why: why, or that what it said did not
/// read, which the engine never writes.
fn unanswered<A>(read: Result<Unanswered, DecodeError>) -> Answered<A> {
    Answered::No(read.unwrap_or(Unanswered::Unreadable))
}
