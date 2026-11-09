//! What comes back from what an actor sends, which its type must handle: the reply a receiver sends to whoever sent
//! to it, checked against the sender where the sender declares the send (ADR-0016).

use super::{Allows, Carries, Handled, Handles, Protocol, Tells, ToAny, ToSender, ToType};
use crate::{Actor, ChildOf, Path, PathTo};
use aethereal_data::Kind;

/// These sends, a tuple of markers, may be declared by an actor of type `A`: what a type's sends are bounded by, so
/// each marker is checked against the type that declares it, where it declares it (ADR-0016, ADR-0022). Chiefly, `A`
/// handles whatever comes back from them, so a type that would be sent a reply it cannot handle does not build.
///
/// A send to an actor of a type brings back the replies that type's handler
/// of the kind declares, and a send through a named protocol what the
/// protocol answers the kind with. A reply, a watch, and a send through a
/// tuple of kinds bring back nothing here. A fallback does not count: a type
/// that takes a reply lists its kind.
///
/// An asker that asks an actor that replies, and does not handle the answer, does not build:
///
/// ```compile_fail
/// use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, ToSender, ToType, Turn};
/// use aethereal_data::Plain;
///
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ask")]
/// # struct Ask;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answer")]
/// # struct Answer;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answerer")]
/// # struct Answerer;
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// /// Answers each question it is asked.
/// impl Handles<Ask> for Answerer {
///     type Sends = (ToSender<Answer>,);
///
///     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Answer)?)
///     }
/// }
///
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.asker")]
/// # struct Asker;
/// #
/// /// Asks as it begins, and handles nothing.
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (ToType<Answerer, Ask>,);
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// So does one that asks from a handler,
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ask")]
/// # struct Ask;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answer")]
/// # struct Answer;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answerer")]
/// # struct Answerer;
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.asker")]
/// # struct Asker;
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = (Ask,);
///     type BeginSends = ();
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Ask> for Asker {
///     type Sends = (ToType<Answerer, Ask>,);
///
///     fn handle(&mut self, _: Ask, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// one that asks as it ends,
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ask")]
/// # struct Ask;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answer")]
/// # struct Answer;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answerer")]
/// # struct Answerer;
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.asker")]
/// # struct Asker;
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = ();
///     type EndSends = (ToType<Answerer, Ask>,);
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// and one that asks through a named protocol that answers what it does not handle:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ask")]
/// # struct Ask;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answer")]
/// # struct Answer;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.answerer")]
/// # struct Answerer;
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.asker")]
/// # struct Asker;
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (ToAny<Asking, Ask>,);
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
pub trait DeclaredBy<A> {}

/// The replies among these sends, a tuple of markers, are each of a kind an actor of type `A` handles: what a send
/// to an actor of a type reads of its receiver's handler (ADR-0016).
pub trait RepliesTo<A> {}

/// An actor of type `A` handles each of these kinds, a tuple of kinds: what a named protocol answers with.
pub trait HandledBy<A> {}

impl<A, T: Actor + Handles<K>, K: Kind + Handled> DeclaredBy<A> for ToType<T, K> where
    <T as Handles<K>>::Sends: RepliesTo<A>
{
}

impl<A, K> DeclaredBy<A> for ToSender<K> {}

/// A send at a typed path is checked as one to the path's type is.
impl<A, L: ChildOf + PathTo + Handles<K>, K: Kind + Handled> DeclaredBy<A> for Tells<Path<L>, K> where
    <L as Handles<K>>::Sends: RepliesTo<A>
{
}

/// A named protocol says what each of its kinds is answered with.
impl<A, P: Protocol + Carries<K>, K: Kind> DeclaredBy<A> for ToAny<P, K> where <P as Carries<K>>::Answers: HandledBy<A> {}

/// A tuple of kinds answers none of them.
impl<A, K> DeclaredBy<A> for ToAny<(), K> {}

impl<A: Handles<K>, K: Kind + Handled> RepliesTo<A> for ToSender<K> {}

impl<A, T, K> RepliesTo<A> for ToType<T, K> {}

impl<A, P, K> RepliesTo<A> for ToAny<P, K> {}

impl<A, T, K> RepliesTo<A> for Tells<T, K> {}

/// This send, of a receiver's handler, replies only with a kind of the tuple `Answers`, found at its place in `I`:
/// what a type that conforms to a named protocol replies with (ADR-0016).
pub trait ReplyWithin<Answers, I> {}

/// The replies among these sends, a tuple of markers, are each of a kind of the tuple `Answers`, found at its place in
/// `Is`.
pub trait RepliesWithin<Answers, Is> {}

/// The replies among these sends, a marker or a tuple of them, are each of the kind `A`: what a type asked at a typed
/// path replies with, when it says it answers the kind asked with `A` alone (ADR-0022).
#[diagnostic::on_unimplemented(
    message = "a handler asked at a typed path replies with a kind its type does not say it answers with",
    label = "replies with something other than `{A}`",
    note = "an actor's type says what it answers each kind it can be asked with `Carries<K>`, and its handler of `K` \
            replies only with that"
)]
pub trait RepliesWith<A> {}

impl<Answers: Allows<R, I>, R, I> ReplyWithin<Answers, I> for ToSender<R> {}

impl<Answers, T, K> ReplyWithin<Answers, ()> for ToType<T, K> {}

impl<Answers, P, K> ReplyWithin<Answers, ()> for ToAny<P, K> {}

impl<Answers, T, K> ReplyWithin<Answers, ()> for Tells<T, K> {}

impl<Answers> RepliesWithin<Answers, ()> for () {}

impl<A> RepliesWith<A> for ToSender<A> {}

impl<A, T, K> RepliesWith<A> for ToType<T, K> {}

impl<A, P, K> RepliesWith<A> for ToAny<P, K> {}

impl<A, T, K> RepliesWith<A> for Tells<T, K> {}

impl<A> RepliesWith<A> for () {}

impl<A> DeclaredBy<A> for () {}

impl<A> RepliesTo<A> for () {}

impl<A> HandledBy<A> for () {}

macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<A, $($name: DeclaredBy<A>),*> DeclaredBy<A> for ($($name,)*) {}

        impl<A, $($name: RepliesTo<A>),*> RepliesTo<A> for ($($name,)*) {}

        impl<A, $($name: RepliesWith<A>),*> RepliesWith<A> for ($($name,)*) {}

        impl<A: $(Handles<$name> +)*, $($name: Kind + Handled),*> HandledBy<A> for ($($name,)*) {}

        impl<A, $($name),*, K> DeclaredBy<A> for ToAny<($($name,)*), K> {}
    )*};
}

macro_rules! within {
    ($( [$($name:ident $is:ident)*] )*) => {$(
        impl<Answers, $($name: ReplyWithin<Answers, $is>, $is),*> RepliesWithin<Answers, ($($is,)*)> for ($($name,)*) {}
    )*};
}

within! {
    [B IB]
    [B IB C IC]
    [B IB C IC D ID]
    [B IB C IC D ID E IE]
    [B IB C IC D ID E IE F IF]
    [B IB C IC D ID E IE F IF G IG]
    [B IB C IC D ID E IE F IF G IG H IH]
    [B IB C IC D ID E IE F IF G IG H IH I II]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62 T63 IT63]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN O IO P IP Q IQ R IR T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62 T63 IT63 T64 IT64]
}

tuples! {
    [B]
    [B C]
    [B C D]
    [B C D E]
    [B C D E F]
    [B C D E F G]
    [B C D E F G H]
    [B C D E F G H I]
    [B C D E F G H I J]
    [B C D E F G H I J L]
    [B C D E F G H I J L M]
    [B C D E F G H I J L M N]
    [B C D E F G H I J L M N O]
    [B C D E F G H I J L M N O P]
    [B C D E F G H I J L M N O P Q]
    [B C D E F G H I J L M N O P Q R]
    [B C D E F G H I J L M N O P Q R T17]
    [B C D E F G H I J L M N O P Q R T17 T18]
    [B C D E F G H I J L M N O P Q R T17 T18 T19]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62 T63]
    [B C D E F G H I J L M N O P Q R T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62 T63 T64]
}
