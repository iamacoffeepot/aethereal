//! A stack's layers, listed as one tuple of types in the order a request passes them (ADR-0035): pure layers and actor
//! layers, in any order, and last the handler. The tuple says what each layer is held as, what the layers attach along
//! it, what the stack's sends list to ask its actor layers, and how the stack walks a request, a response, and each
//! part of a body through them.

use super::actor::{Parked, ParkedResponse};
use super::walks::{Ask, Formed, Returned, Taker, Unasked, Walked, Walks, unlayered};
use super::{BodyPart, Head, Layer, LayerForm, Leaving, Response, StackSends};
use crate::Status;
use aethereal_actor::{Actor, ActorRef, At, Beginning, ChildOf, Fault, Lists, PathTo, Time, Turn, Within, path};
use aethereal_data::{Bound, Decode, Encode, KindName, Plain};
use core::any::Any;

/// A stack's layers, as one tuple of types: up to five layers, pure or actor, in any order, each whose `In` is the
/// `Out` of the one before it, the first's `()`; and last its handler, an actor that is a singleton root, asked a
/// [`Request`](super::Request) of what the layers attached. A pure layer's `In` and `Out` are its [`Layer`]'s, and an
/// actor layer's those of the [`Inbound`](super::Inbound) it handles and the [`Inward`](super::Inward) it says it
/// answers it with.
pub trait LayerList: 'static {
    /// The tuple of its layers' forms: what the stack holds to use them, written out by the compiler from the list.
    type Forms: Encode<Within> + Decode<Within> + Clone;
    /// The handler: the last layer, which answers.
    type Handler: Actor;
    /// What the layers attach to a request, which the handler is asked with.
    type Attached: Plain;
    /// What the stack's sends list to ask its actor layers, ten in all: for each place before the handler, in order,
    /// its ask on the way in and its ask on the way out, [`Unasked`] for a pure layer and for each place past the last.
    type Asked: Slot<At<0>>
        + Slot<At<1>>
        + Slot<At<2>>
        + Slot<At<3>>
        + Slot<At<4>>
        + Slot<At<5>>
        + Slot<At<6>>
        + Slot<At<7>>
        + Slot<At<8>>
        + Slot<At<9>>;
    /// How many layers stand before the handler.
    const LAYERS: usize;

    /// Each layer as the stack holds it: a pure layer made with `Default`, and an actor layer and the handler found
    /// at their typed paths as `turn` sees the engine.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`], naming the type of an actor layer or the handler that is not live.
    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault>;

    /// The handler, as `forms` hold it.
    fn handler(forms: &Self::Forms) -> ActorRef<Self::Handler>;

    /// Passes `head` in through each layer of `forms`, in order, from the first: what they attached; the response of
    /// the one that answered; or the actor layer it is parked at, asked keeping `parked` until it answers by `due`.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn inbound<Me>(
        forms: &mut Self::Forms,
        head: &Head,
        turn: &mut Turn<'_, Me, StackSends<Self>>,
        parked: Parked,
        due: Time,
    ) -> Result<Entered<Self::Attached>, Fault>;

    /// Passes `head` on in from after the actor layer at `at`, which passed it with `attached`: as
    /// [`inbound`](Self::inbound) does from there. `attached` is of the type the layer passes on, which its answer's
    /// kind says; were it not, the request would be answered `500` there.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn resume<Me, X: 'static>(
        forms: &mut Self::Forms,
        at: usize,
        attached: X,
        head: &Head,
        turn: &mut Turn<'_, Me, StackSends<Self>>,
        parked: Parked,
        due: Time,
    ) -> Result<Entered<Self::Attached>, Fault>;

    /// Hands the layer at `at` of `forms` a response on its way out to the request `head`: an actor layer is asked,
    /// keeping `parked` until it answers by `due`.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn outbound<Me>(
        forms: &mut Self::Forms,
        at: usize,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me, StackSends<Self>>,
        parked: ParkedResponse,
        due: Time,
    ) -> Result<Returned, Fault>;

    /// What the layer at `at` of `forms` is, as a part of a body reaches it.
    fn taker(forms: &Self::Forms, at: usize) -> Taker;

    /// Hands the pure layer at `at` of `forms` a part of the request's body.
    fn inbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>);

    /// Hands the pure layer at `at` of `forms` a part of the response's body.
    fn outbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>);
}

/// What became of a request on its way in through a stack's layers.
#[derive(Debug)]
pub enum Entered<A> {
    /// Every layer passed it, attaching `A`: it goes on to the handler.
    Passed(A),
    /// The layer at `at` answered it with `response`, which turns back through the layers before that one.
    Answered { at: usize, response: Response },
    /// The actor layer at `at` was asked about it, and it is parked until the layer answers.
    Parked { at: usize },
}

/// Why a stack did not begin: a layer it lists is an actor that is not live, as a root listed after the stack, or
/// one that has ended.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.unlayered")]
pub struct Unlayered {
    /// The layer's type.
    pub layer: KindName,
}

/// A stack's layers, as `layers` reads them from the type it is to answer, its pure layers' template set by type with
/// [`layer_mut`](Self::layer_mut).
#[derive(Bound)]
pub struct Layers<T: LayerList> {
    forms: T::Forms,
}

/// The stack's layers, `T`, read from the annotation, each built as what it is: a pure layer with `Default`, and an
/// actor layer and the handler found at their typed paths with the begin's turn.
///
/// # Errors
///
/// The fault [`Unlayered`], naming the type of an actor layer or the handler that is not live.
pub fn layers<T: LayerList, Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Layers<T>, Fault> {
    Ok(Layers { forms: T::found(turn)? })
}

impl<T: LayerList> Layers<T> {
    /// The pure layer of type `L`, found at its place, `I`, which the compiler infers: a type listed twice is
    /// ambiguous, and does not build.
    pub fn layer_mut<L: Layer, I>(&mut self) -> &mut L
    where
        T: Lists<L, I>,
        T::Forms: Slot<I, Item = L>,
    {
        self.forms.slot()
    }

    /// A set of each layer's form, made from the template as it stands: a new connection's.
    pub(super) fn made(&self) -> T::Forms {
        self.forms.clone()
    }
}

/// What an actor layer passed on, `attached`, as `Y`, the type the layer after it takes: the type its place in the
/// list says, which the answer's kind, `X`, is, so it is found; none were they ever to differ.
fn as_taken<X: 'static, Y: 'static>(attached: X) -> Option<Y> {
    let mut carried = Some(attached);
    let as_any: &mut dyn Any = &mut carried;

    as_any.downcast_mut::<Option<Y>>().and_then(Option::take)
}

/// A request answered `500` at the layer `at`, as one whose actor layer passed on what the next does not take.
fn untaken<A>(at: usize) -> Entered<A> {
    Entered::Answered { at, response: Response::empty(Status::InternalServerError) }
}

/// The element of a tuple at `I`, an [`At`] the compiler infers alongside [`Lists`], reached: `Lists` locates a type in
/// a tuple of types, and this reaches the value at that place in a tuple of values.
pub trait Slot<I> {
    /// What stands there.
    type Item;

    /// What stands there, to change.
    fn slot(&mut self) -> &mut Self::Item;
}

macro_rules! slots {
    ($( [$($name:ident $place:tt)*] )*) => {$(
        slots!(@slots [$($name)*] $($name $place)*);
    )*};
    (@slots $all:tt $($name:ident $place:tt)*) => {$(
        slots!(@slot $all $name $place);
    )*};
    (@slot [$($all:ident)*] $name:ident $place:tt) => {
        impl<$($all),*> Slot<At<$place>> for ($($all,)*) {
            type Item = $name;

            fn slot(&mut self) -> &mut $name {
                &mut self.$place
            }
        }
    };
}

slots! {
    [T0 0]
    [T0 0 T1 1]
    [T0 0 T1 1 T2 2]
    [T0 0 T1 1 T2 2 T3 3]
    [T0 0 T1 1 T2 2 T3 3 T4 4]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9]
}

/// A handler alone, with no layer before it, is handed every request as it came, with nothing attached.
impl<H> LayerList for (H,)
where
    H: Actor + ChildOf + PathTo<Keys = ()>,
{
    type Forms = (<H as LayerForm>::Form,);
    type Handler = H;
    type Attached = ();
    type Asked = (Unasked, Unasked, Unasked, Unasked, Unasked, Unasked, Unasked, Unasked, Unasked, Unasked);
    const LAYERS: usize = 0;

    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
        let handler = turn.found(&path::<H>(())).ok_or_else(unlayered::<H>)?;

        Ok((handler,))
    }

    fn handler(forms: &Self::Forms) -> ActorRef<H> {
        forms.0
    }

    fn inbound<Me>(
        _: &mut Self::Forms,
        _: &Head,
        _: &mut Turn<'_, Me, StackSends<Self>>,
        _: Parked,
        _: Time,
    ) -> Result<Entered<()>, Fault> {
        Ok(Entered::Passed(()))
    }

    fn resume<Me, X: 'static>(
        _: &mut Self::Forms,
        at: usize,
        _: X,
        _: &Head,
        _: &mut Turn<'_, Me, StackSends<Self>>,
        _: Parked,
        _: Time,
    ) -> Result<Entered<()>, Fault> {
        Ok(untaken(at))
    }

    fn outbound<Me>(
        _: &mut Self::Forms,
        _: usize,
        _: &Head,
        _: &mut Leaving<'_>,
        _: &mut Turn<'_, Me, StackSends<Self>>,
        _: ParkedResponse,
        _: Time,
    ) -> Result<Returned, Fault> {
        Ok(Returned::Passed)
    }

    fn taker(_: &Self::Forms, _: usize) -> Taker {
        Taker::Pure
    }

    fn inbound_body(_: &mut Self::Forms, _: usize, _: &mut BodyPart<'_>) {}

    fn outbound_body(_: &mut Self::Forms, _: usize, _: &mut BodyPart<'_>) {}
}

/// Layers, each named with its place and the places in the stack's sends of its asks in and out, then the handler with
/// its place, and an [`Unasked`] for each place of the ten past the last layer's. The bounds that chain each layer's
/// `In` to the `Out` before it are gathered one layer at a time, carrying what is attached so far, and each layer
/// keeps the `In` it takes.
macro_rules! layered {
    ($( [$($layer:ident $at:tt $in:tt $out:tt)+] $handler:ident $last:tt [$($unasked:ident)*]; )*) => {$(
        layered!(@chain [] [()] [] [$($layer $at $in $out)+] $handler $last [$($unasked)*]);
    )*};
    (
        @chain [$($bounds:tt)*] [$attached:ty] [$($walked:tt)*] [$layer:ident $at:tt $in:tt $out:tt $($rest:tt)*]
        $handler:ident $last:tt $unasked:tt
    ) => {
        layered!(
            @chain
            [$($bounds)* $layer: LayerForm + 'static, <$layer as LayerForm>::Form: Walks<$attached>,]
            [<<$layer as LayerForm>::Form as Walks<$attached>>::Out]
            [$($walked)* [$layer $at $in $out [$attached]]]
            [$($rest)*]
            $handler
            $last
            $unasked
        );
    };
    (
        @chain [$($bounds:tt)*] [$attached:ty] [$([$layer:ident $at:tt $in:tt $out:tt [$taken:ty]])+] []
        $handler:ident $last:tt [$($unasked:ident)*]
    ) => {
        impl<$($layer,)+ $handler> LayerList for ($($layer,)+ $handler,)
        where
            $($bounds)*
            $handler: Actor + ChildOf + PathTo<Keys = ()>,
        {
            type Forms = ($(<$layer as LayerForm>::Form,)+ <$handler as LayerForm>::Form,);
            type Handler = $handler;
            type Attached = $attached;
            type Asked = (
                $(
                    <<$layer as LayerForm>::Form as Walks<$taken>>::Inbound,
                    <<$layer as LayerForm>::Form as Formed>::Outbound,
                )+
                $($unasked,)*
            );
            const LAYERS: usize = $last;

            fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
                let handler = turn.found(&path::<$handler>(())).ok_or_else(unlayered::<$handler>)?;

                Ok(($(<<$layer as LayerForm>::Form as Formed>::found(turn)?,)+ handler,))
            }

            fn handler(forms: &Self::Forms) -> ActorRef<$handler> {
                forms.$last
            }

            fn inbound<Me>(
                forms: &mut Self::Forms,
                head: &Head,
                turn: &mut Turn<'_, Me, StackSends<Self>>,
                parked: Parked,
                due: Time,
            ) -> Result<Entered<$attached>, Fault> {
                let attached = ();

                layered!(@walk forms head turn parked due attached [$([$layer $at $in $out [$taken]])+])
            }

            #[allow(unused_variables, reason = "a list of one layer resumes at its handler, and walks no layer more")]
            fn resume<Me, X: 'static>(
                forms: &mut Self::Forms,
                at: usize,
                attached: X,
                head: &Head,
                turn: &mut Turn<'_, Me, StackSends<Self>>,
                parked: Parked,
                due: Time,
            ) -> Result<Entered<$attached>, Fault> {
                layered!(@resume forms at head turn parked due attached [$([$layer $at $in $out [$taken]])+]);

                Ok(untaken(at))
            }

            fn outbound<Me>(
                forms: &mut Self::Forms,
                at: usize,
                head: &Head,
                leaving: &mut Leaving<'_>,
                turn: &mut Turn<'_, Me, StackSends<Self>>,
                parked: ParkedResponse,
                due: Time,
            ) -> Result<Returned, Fault> {
                $(
                    if at == $at {
                        let ask: Ask<At<$out>, ParkedResponse> = Ask::new(parked.at($at), due);
                        return <<$layer as LayerForm>::Form as Formed>::outbound(
                            &mut forms.$at,
                            head,
                            leaving,
                            turn,
                            ask,
                        );
                    }
                )+

                Ok(Returned::Passed)
            }

            fn taker(forms: &Self::Forms, at: usize) -> Taker {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::taker(&forms.$at),)+
                    _ => Taker::Pure,
                }
            }

            fn inbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::inbound_body(&mut forms.$at, part),)+
                    _ => {}
                }
            }

            fn outbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::outbound_body(&mut forms.$at, part),)+
                    _ => {}
                }
            }
        }
    };
    (@walk $forms:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident []) => {
        Ok(Entered::Passed($attached))
    };
    (
        @walk $forms:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident
        [[$layer:ident $at:tt $in:tt $out:tt [$taken:ty]] $($rest:tt)*]
    ) => {{
        let ask: Ask<At<$in>, Parked> = Ask::new($parked.at($at), $due);
        let walked =
            <<$layer as LayerForm>::Form as Walks<$taken>>::inbound(&mut $forms.$at, $head, $attached, $turn, ask)?;
        let $attached = match walked {
            Walked::Passed(attached) => attached,
            Walked::Answered(response) => return Ok(Entered::Answered { at: $at, response }),
            Walked::Parked => return Ok(Entered::Parked { at: $at }),
        };

        layered!(@walk $forms $head $turn $parked $due $attached [$($rest)*])
    }};
    (@resume $forms:ident $from:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident []) => {};
    (
        @resume $forms:ident $from:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident
        [[$layer:ident $at:tt $in:tt $out:tt [$taken:ty]] $($rest:tt)*]
    ) => {
        if $from == $at {
            let taken: Option<<<$layer as LayerForm>::Form as Walks<$taken>>::Out> = as_taken($attached);
            let Some(taken) = taken else {
                return Ok(untaken($at));
            };

            return layered!(@walk $forms $head $turn $parked $due taken [$($rest)*]);
        }

        layered!(@resume $forms $from $head $turn $parked $due $attached [$($rest)*]);
    };
}

layered! {
    [L0 0 12 13] H 1 [Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [L0 0 12 13 L1 1 14 15] H 2 [Unasked Unasked Unasked Unasked Unasked Unasked];
    [L0 0 12 13 L1 1 14 15 L2 2 16 17] H 3 [Unasked Unasked Unasked Unasked];
    [L0 0 12 13 L1 1 14 15 L2 2 16 17 L3 3 18 19] H 4 [Unasked Unasked];
    [L0 0 12 13 L1 1 14 15 L2 2 16 17 L3 3 18 19 L4 4 20 21] H 5 [];
}
