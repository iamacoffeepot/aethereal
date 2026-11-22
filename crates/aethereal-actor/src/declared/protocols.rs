//! What a reference typed by a protocol is proven to reach: the protocols, a type that conforms to one, and a protocol
//! that covers another (ADR-0016).

use super::{Handled, Handles, Lists};
use crate::answering::{Answering, Answers};
use aethereal_data::{Answered, Kind, KindId, KindPtr, ProtocolShape};

/// A named protocol: a list of the kinds it takes, and what each is answered with, one [`Carries`] for each, as an
/// actor lists what it handles and writes one handler for each (ADR-0016).
///
/// ```
/// use aethereal_actor::{Carries, Protocol};
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.which_tile")]
/// struct WhichTile;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.tile_is")]
/// struct TileIs;
///
/// struct Tiles;
///
/// impl Protocol for Tiles {
///     type Kinds = (WhichTile,);
/// }
///
/// impl Carries<WhichTile> for Tiles {
///     type Answers = (TileIs,);
/// }
/// ```
///
/// A kind it lists with no [`Carries`] does not build:
///
/// ```compile_fail
/// use aethereal_actor::Protocol;
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.highlight")]
/// struct Highlight;
///
/// struct Tiles;
///
/// impl Protocol for Tiles {
///     type Kinds = (Highlight,);
/// }
/// ```
pub trait Protocol: Sized {
    /// The kinds it takes: a tuple of up to 64 kinds.
    type Kinds: Kinds + CarriedBy<Self>;
}

/// What a named protocol, or an actor's type, answers mail of kind `K` with: `Answers`, `(A,)` for a request answered
/// with `A`, `()` for a tell (ADR-0016, ADR-0017).
///
/// An actor's type carries each kind it handles as its handler returns, [`Handles::Returns`], and nothing writes it
/// by hand (ADR-0059). A named protocol, which has no handler, writes one for each kind it lists.
pub trait Carries<K: Kind> {
    /// The one kind the receiver answers with, or none.
    type Answers: Answers;
}

/// An actor's type answers each kind it handles as its handler returns.
impl<T: Handles<K>, K: Kind> Carries<K> for T {
    type Answers = <T::Returns as Answering>::Answers;
}

/// The protocol `P` has a [`Carries`] for each kind of this tuple: what makes a named protocol's list its own.
pub trait CarriedBy<P> {
    /// Each kind, in order, with the kinds `P` carries it as answered with.
    const ANSWERED: &'static [Answered];
}

/// A protocol a [`ProtocolRef`](crate::ProtocolRef) reaches an actor by: a tuple of kinds, which are all tells, or a
/// named [`Protocol`] (ADR-0016).
pub trait Listed {
    /// Its kinds, as a tuple.
    type Kinds: Kinds;
    /// What the layout of a reference to it names: its kinds, and for a named protocol what each is answered with
    /// (ADR-0016).
    const PROTOCOL: ProtocolShape;
}

impl<P: Protocol> Listed for P {
    type Kinds = P::Kinds;
    const PROTOCOL: ProtocolShape = ProtocolShape::Named(<P::Kinds as CarriedBy<P>>::ANSWERED);
}

impl Listed for () {
    type Kinds = ();
    const PROTOCOL: ProtocolShape = ProtocolShape::Told(&[]);
}

impl<P> CarriedBy<P> for () {
    const ANSWERED: &'static [Answered] = &[];
}

/// A tuple of up to 64 kinds, as a protocol lists them, `()` for none: what a [`Listed`] protocol names its kinds by,
/// and the engine checks a cast against.
pub trait Kinds {
    /// The ids of the kinds, in order.
    const KINDS: &'static [KindId];
    /// The nodes of the kinds, in order: what a reference's layout reaches them by, so a kind may hold a reference to a
    /// protocol that takes it (ADR-0006, ADR-0016).
    const NODES: &'static [KindPtr];
}

impl Kinds for () {
    const KINDS: &'static [KindId] = &[];
    const NODES: &'static [KindPtr] = &[];
}

/// An actor of this type takes every kind of the protocol `P`: an [`ActorRef`](crate::ActorRef) to one is made a
/// [`ProtocolRef<P>`](crate::ProtocolRef) by its `protocol` (ADR-0016).
///
/// It conforms when it handles each kind of `P`, and answers each kind a named protocol carries as the protocol says
/// (ADR-0059). The compiler checks it, and nothing is read when the program runs.
///
/// A board that does not handle a kind of the protocol does not conform:
///
/// ```compile_fail
/// # use aethereal_actor::{ActorRef, Fault, ProtocolRef, actor, protocol};
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
/// # #[protocol(WhichTile(answers = TileIs))]
/// # struct Tiles;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.board")]
/// # struct Board;
/// #
/// /// Handles nothing, so takes no `WhichTile`.
/// #[actor]
/// impl Board {
///     fn begin(board: Board) -> Result<Self, Fault> {
///         Ok(board)
///     }
/// }
///
/// fn tiles(board: ActorRef<Board>) -> ProtocolRef<Tiles> {
///     board.protocol()
/// }
/// ```
///
/// A board whose handler of a kind of a named protocol answers it with another kind than the protocol carries does not
/// conform:
///
/// ```compile_fail,E0271
/// # use aethereal_actor::{ActorRef, Fault};
/// # use aethereal_actor::{ProtocolRef, actor, protocol};
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
/// # #[kind(name = "aether.example.no_tile")]
/// # struct NoTile;
/// #
/// # #[protocol(WhichTile(answers = TileIs))]
/// # struct Tiles;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.board")]
/// # struct Board;
/// #
/// #[actor]
/// impl Board {
/// #     fn begin(board: Board) -> Result<Self, Fault> {
/// #         Ok(board)
/// #     }
/// #
///     /// Answers `WhichTile` with a `NoTile`, where `Tiles` carries it as answered with a `TileIs`.
///     #[request]
///     fn which_tile(&mut self, _: WhichTile) -> Result<NoTile, Fault> {
///         Ok(NoTile)
///     }
/// }
///
/// fn tiles(board: ActorRef<Board>) -> ProtocolRef<Tiles> {
///     board.protocol()
/// }
/// ```
pub trait Conforms<P> {}

/// The actor of type `T` handles each of these kinds, the kinds of the protocol `P`, and answers each as `P` does.
pub trait HandledBy<T, P> {}

impl<T, P: Listed> Conforms<P> for T where P::Kinds: HandledBy<T, P> {}

/// The protocol answers mail of kind `K` as an actor of type `T` does: a named protocol with the kind `T`'s handler
/// answers, and a tuple of kinds, whose kinds are told, whatever `T` answers.
pub trait AnsweredAlike<T, K> {}

impl<T, P, K> AnsweredAlike<T, K> for P
where
    P: Protocol + Carries<K>,
    T: Carries<K, Answers = <P as Carries<K>>::Answers>,
    K: Kind,
{
}

impl<T, K> AnsweredAlike<T, K> for () {}

/// The protocol takes every kind of the tuple of kinds `Q`, each found at its place in `Is`, which the compiler infers:
/// a [`ProtocolRef`](crate::ProtocolRef) to it narrows to one to `Q` (ADR-0016).
///
/// The kinds may be in any order, so `(A, B)` covers `(B, A)`. A kind found twice is not inferred, and does not
/// build.
pub trait Covers<Q, Is> {}

impl<T, P> HandledBy<T, P> for () {}

impl<P> Covers<(), ()> for P {}

macro_rules! protocols {
    ($( [$($kind:ident $is:ident)*] )*) => {$(
        impl<T: $(Handles<$kind> +)*, P: $(AnsweredAlike<T, $kind> +)*, $($kind: Kind + Handled),*> HandledBy<T, P>
            for ($($kind,)*)
        {
        }

        impl<T, Told, $($kind),*> AnsweredAlike<T, Told> for ($($kind,)*) {}

        impl<P, $($kind, $is),*> Covers<($($kind,)*), ($($is,)*)> for P where $(P: Lists<$kind, $is>),* {}

        impl<$($kind: Kind),*> Kinds for ($($kind,)*) {
            const KINDS: &'static [KindId] = &[$(KindId::of::<$kind>()),*];
            const NODES: &'static [KindPtr] = &[$($kind::KIND),*];
        }

        impl<$($kind: Kind),*> Listed for ($($kind,)*) {
            type Kinds = Self;
            const PROTOCOL: ProtocolShape = ProtocolShape::Told(<Self as Kinds>::NODES);
        }

        impl<P: $(Carries<$kind> +)*, $($kind: Kind),*> CarriedBy<P> for ($($kind,)*) {
            const ANSWERED: &'static [Answered] = &[$(Answered {
                kind: $kind::KIND,
                answers: <<P as Carries<$kind>>::Answers as Kinds>::NODES,
            }),*];
        }
    )*};
}

protocols! {
    [A IA]
    [A IA B IB]
    [A IA B IB C IC]
    [A IA B IB C IC D ID]
    [A IA B IB C IC D ID E IE]
    [A IA B IB C IC D ID E IE F IF]
    [A IA B IB C IC D ID E IE F IF G IG]
    [A IA B IB C IC D ID E IE F IF G IG H IH]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62 T63 IT63]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM N IN O IO Q IQ T17 IT17 T18 IT18 T19 IT19 T20 IT20 T21 IT21 T22 IT22 T23 IT23 T24 IT24 T25 IT25 T26 IT26 T27 IT27 T28 IT28 T29 IT29 T30 IT30 T31 IT31 T32 IT32 T33 IT33 T34 IT34 T35 IT35 T36 IT36 T37 IT37 T38 IT38 T39 IT39 T40 IT40 T41 IT41 T42 IT42 T43 IT43 T44 IT44 T45 IT45 T46 IT46 T47 IT47 T48 IT48 T49 IT49 T50 IT50 T51 IT51 T52 IT52 T53 IT53 T54 IT54 T55 IT55 T56 IT56 T57 IT57 T58 IT58 T59 IT59 T60 IT60 T61 IT61 T62 IT62 T63 IT63 T64 IT64]
}
