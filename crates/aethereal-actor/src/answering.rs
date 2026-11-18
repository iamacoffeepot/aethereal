//! How a handler of mail answers it: what it returns, which the engine sends (ADR-0017, ADR-0059).
//!
//! A handler of a kind returns one of four forms, and its type is the actor's answer to the kind, its
//! [`Carries`](crate::Carries):
//!
//! - `()` for a tell, which answers nothing;
//! - the answer `A`, sent now;
//! - [`Later<A>`], which only [`Turn::hold`](crate::Turn::hold) makes, so a handler that returns one has held its
//!   request, and answers it in a later turn through the [`Held<A>`](crate::Held) made beside it;
//! - [`Reply<A>`], either, as the handler's body decides.
//!
//! On `Ok`, the engine sends the answer given now, and nothing for one given later. A handler has no other way to
//! answer the request it handles.

use crate::guard::Guard;
use crate::{Actors, Doing, Kinds};
use aethereal_data::{Encode, EncodeError, IntoKinds, Kind};
use core::any;
use core::fmt;
use core::marker::PhantomData;

/// A request answered later: what [`Turn::hold`](crate::Turn::hold) makes beside the [`Held<A>`](crate::Held) it is
/// answered through, and what the request's handler returns, so a handler that returns one has held its request
/// (ADR-0017, ADR-0059).
///
/// Only a hold makes one:
///
/// ```compile_fail,E0451
/// use aethereal_actor::Later;
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.fetched")]
/// # struct Fetched;
///
/// fn unheld() -> Later<Fetched> {
///     Later { of: core::marker::PhantomData, unsent: core::marker::PhantomData }
/// }
/// ```
///
/// It cannot be kept past the turn that made it: it is neither `Clone` nor `Send`, and is written to no bytes, so
/// neither an actor's state, which is `Send` and written, nor what it keeps for a request of its own can hold it. A
/// turn handles one item, so the `Later` a request's handler returns is its own request's. A turn that holds what it
/// does not owe, its item no request, or one it has held or answered already, fails with
/// [`NotOwed`](crate::NotOwed) once its handler returns.
#[must_use = "a request's handler that holds returns the hold's `Later`"]
pub struct Later<A> {
    of: PhantomData<fn() -> A>,
    /// What keeps it from being `Send`, so no actor's state holds it.
    unsent: PhantomData<*const ()>,
}

impl<A> Later<A> {
    /// What a hold makes.
    pub(crate) const fn new() -> Self {
        Self { of: PhantomData, unsent: PhantomData }
    }
}

impl<A> fmt::Debug for Later<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Later<{}>", any::type_name::<A>())
    }
}

/// A request answered now or later, as its handler decides in its body (ADR-0059).
#[derive(Debug)]
pub enum Reply<A> {
    /// The answer, sent once the handler returns.
    Now(A),
    /// Answered in a later turn, through the hold that made it.
    Later(Later<A>),
}

/// What a handler of mail returns, which says how it answers: `()`, an answer `A`, a [`Later<A>`], or a [`Reply<A>`]
/// (ADR-0059). The engine sends what it answers now, once the handler returns `Ok`.
pub trait Answering: Sized {
    /// What the kind handled is answered with: `()` for nothing, `(A,)` for one kind `A`. An actor's
    /// [`Carries`](crate::Carries) of the kind is this.
    type Answers: Answers;

    /// Sends the answer it gives now to the asker of the request `doing` handles, when its item is a request: what the
    /// engine does once the handler returns, and nothing else can, as only this crate makes a [`Token`].
    ///
    /// # Errors
    ///
    /// Why the answer could not be written, or that the request was held or answered already.
    fn send<Me, F: Guard<Me>>(self, doing: &mut Doing<'_, Me, F>, token: Token) -> Result<(), EncodeError>;
}

/// What a kind is answered with: no kind, `()`, or one kind, `(A,)` (ADR-0017). A request is answered with one kind, so
/// no other tuple is an answer.
pub trait Answers: Kinds + sealed::Sealed {
    /// Sends the answer, when it is one, to the asker of the request `doing` handles: what the engine does once the
    /// handler returns.
    ///
    /// # Errors
    ///
    /// As [`Answering::send`].
    fn send<Me, F: Guard<Me>>(self, doing: &mut Doing<'_, Me, F>, token: Token) -> Result<(), EncodeError>;
}

/// What only this crate makes: the proof that an answer is sent by the engine, as its handler returns.
pub struct Token(());

impl Token {
    /// The engine's, as it sends a handler's answer.
    pub(crate) const fn new() -> Self {
        Self(())
    }
}

impl<R: IntoKinds> Answering for R
where
    R::Kinds: Answers,
{
    type Answers = R::Kinds;

    #[inline]
    fn send<Me, F: Guard<Me>>(self, doing: &mut Doing<'_, Me, F>, token: Token) -> Result<(), EncodeError> {
        self.into_kinds().send(doing, token)
    }
}

impl<A: Kind + Encode<Actors>> Answering for Later<A> {
    type Answers = (A,);

    /// The answer is sent later, through the hold that made it.
    #[inline]
    fn send<Me, F: Guard<Me>>(self, _: &mut Doing<'_, Me, F>, _: Token) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<A: Kind + Encode<Actors>> Answering for Reply<A> {
    type Answers = (A,);

    #[inline]
    fn send<Me, F: Guard<Me>>(self, doing: &mut Doing<'_, Me, F>, token: Token) -> Result<(), EncodeError> {
        match self {
            Self::Now(answer) => (answer,).send(doing, token),
            Self::Later(_) => Ok(()),
        }
    }
}

impl Answers for () {
    #[inline]
    fn send<Me, F: Guard<Me>>(self, _: &mut Doing<'_, Me, F>, _: Token) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<A: Kind + Encode<Actors>> Answers for (A,) {
    #[inline]
    fn send<Me, F: Guard<Me>>(self, doing: &mut Doing<'_, Me, F>, _: Token) -> Result<(), EncodeError> {
        doing.answer_now(&self.0)
    }
}

/// What a handler makes of mail its guard refused: an answer in its handler's form, or the default (ADR-0058,
/// ADR-0059).
#[derive(Debug)]
pub enum Refusal<R> {
    /// Answered as its handler answers, in its form, which the engine sends as it sends the handler's.
    Answers(R),
    /// The default: the refusal is logged, naming the sender, a request answered
    /// [`Unanswered::Unreadable`](crate::Unanswered::Unreadable), and a tell dropped.
    Logged,
}

mod sealed {
    use aethereal_data::Kind;

    /// What only this crate implements.
    pub trait Sealed {}

    impl Sealed for () {}

    impl<A: Kind> Sealed for (A,) {}
}
