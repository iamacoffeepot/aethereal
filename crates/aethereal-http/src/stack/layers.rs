//! A stack's layers, listed as one tuple of types in the order a request passes them (ADR-0035): pure layers and actor
//! layers, in any order, and last the layer that answers, a handler or a routes type. The tuple says what each layer is
//! held as, what the layers attach along it, what the stack's sends list to ask its actor layers and its handlers, and
//! how the stack walks a request, a response, and each part of a body through them.

use super::actor::{Parked, ParkedResponse};
use super::last::LastLayer;
use super::walks::{Ask, Formed, Returned, Taker, Walked, Walks};
use super::{BodyPart, Head, Layer, LayerForm, Leaving, Response, StackSends};
use crate::Status;
use aethereal_actor::{At, Beginning, Fault, Lists, Time, Turn, Within};
use aethereal_data::{Bound, Decode, Encode, KindName, Plain};
use core::any::Any;

/// A stack's layers, as one tuple of types: up to five layers, pure or actor, in any order, each whose `In` is the
/// `Out` of the one before it, the first's `()`; and last the layer that answers, a handler or a routes type, whose
/// handlers are asked a [`Request`](super::Request) of what the layers attached. A pure layer's `In` and `Out` are its
/// [`Layer`]'s, and an actor layer's those of the [`Inbound`](super::Inbound) it handles and the
/// [`Inward`](super::Inward) it says it answers it with.
pub trait LayerList: 'static {
    /// The tuple of its layers' forms, the last's with them: the template the stack holds, written out by the compiler
    /// from the list.
    type Forms: Encode<Within> + Decode<Within>;
    /// The forms of the layers before the last: the set each connection holds of its own, made from the template.
    type Values: Encode<Within> + Decode<Within>;
    /// The last layer, as the stack holds it: the handler, or the routes' router.
    type Last: LastLayer<Self::Attached>;
    /// What the layers attach to a request, which its handler is asked with.
    type Attached: Plain;
    /// What the stack's sends list to ask its actor layers and its handlers, twelve in all: for each layer before the
    /// last, in order, its ask on the way in and its ask on the way out, [`Unasked`](super::Unasked) for a pure layer;
    /// then the last layer's ask of each of its handlers, as many as the places left.
    type Asked: Slot<At<0>>
        + Slot<At<1>>
        + Slot<At<2>>
        + Slot<At<3>>
        + Slot<At<4>>
        + Slot<At<5>>
        + Slot<At<6>>
        + Slot<At<7>>
        + Slot<At<8>>
        + Slot<At<9>>
        + Slot<At<10>>
        + Slot<At<11>>;
    /// How many layers stand before the last.
    const LAYERS: usize;

    /// Each layer as the stack holds it: a pure layer made with `Default`, an actor layer and a lone handler found at
    /// their typed paths as `turn` sees the engine, and a routes type's router compiled from its routes.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`], naming the type of an actor layer or a lone handler that is not live; or
    /// [`Unroutable`](crate::Unroutable), when a routes type's routes are refused.
    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault>;

    /// A connection's own set of the forms of the layers before the last, made from `forms` as they stand.
    fn made(forms: &Self::Forms) -> Self::Values;

    /// The last layer, as `forms` hold it.
    fn last(forms: &Self::Forms) -> &Self::Last;

    /// Passes `head` in through each layer of `values`, in order, from the first: what they attached; the response of
    /// the one that answered; or the actor layer it is parked at, asked keeping `parked` until it answers by `due`.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn inbound<Me, Own>(
        values: &mut Self::Values,
        head: &Head,
        turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
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
    fn resume<Me, X: 'static, Own>(
        values: &mut Self::Values,
        at: usize,
        attached: X,
        head: &Head,
        turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
        parked: Parked,
        due: Time,
    ) -> Result<Entered<Self::Attached>, Fault>;

    /// Hands the layer at `at` of `values` a response on its way out to the request `head`: an actor layer is asked,
    /// keeping `parked` until it answers by `due`.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn outbound<Me, Own>(
        values: &mut Self::Values,
        at: usize,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
        parked: ParkedResponse,
        due: Time,
    ) -> Result<Returned, Fault>;

    /// What the layer at `at` of `values` is, as a part of a body reaches it.
    fn taker(values: &Self::Values, at: usize) -> Taker;

    /// Hands the pure layer at `at` of `values` a part of the request's body.
    fn inbound_body(values: &mut Self::Values, at: usize, part: &mut BodyPart<'_>);

    /// Hands the pure layer at `at` of `values` a part of the response's body.
    fn outbound_body(values: &mut Self::Values, at: usize, part: &mut BodyPart<'_>);
}

/// What became of a request on its way in through a stack's layers.
#[derive(Debug)]
pub enum Entered<A> {
    /// Every layer passed it, attaching `A`: it goes on to the last layer.
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

/// The stack's layers, `T`, read from the annotation, each built as what it is: a pure layer with `Default`, an actor
/// layer and a lone handler found at their typed paths with the begin's turn, and a routes type's router compiled from
/// its routes.
///
/// # Errors
///
/// The fault [`Unlayered`], naming the type of an actor layer or a lone handler that is not live; or
/// [`Unroutable`](crate::Unroutable), when a routes type's routes are refused.
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

    /// A set of the forms of the layers before the last, made from the template as it stands: a new connection's.
    pub(super) fn made(&self) -> T::Values {
        T::made(&self.forms)
    }

    /// The last layer, which every connection shares.
    pub(super) fn last(&self) -> &T::Last {
        T::last(&self.forms)
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
    /// Where it stands.
    const PLACE: usize;

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
            const PLACE: usize = $place;

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
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11]
}

/// The last layer alone, with no layer before it: its handlers are handed every request as it came, with nothing
/// attached, and it has every place of the stack's sends for them.
impl<L> LayerList for (L,)
where
    L: LayerForm + 'static,
    L::Form: LastLayer<()>,
{
    type Forms = (L::Form,);
    type Values = ();
    type Last = L::Form;
    type Attached = ();
    type Asked = <L::Form as LastLayer<()>>::Asked;
    const LAYERS: usize = 0;

    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
        Ok((L::Form::found(turn)?,))
    }

    fn made(_: &Self::Forms) -> Self::Values {}

    fn last(forms: &Self::Forms) -> &Self::Last {
        &forms.0
    }

    fn inbound<Me, Own>(
        (): &mut Self::Values,
        _: &Head,
        _: &mut Turn<'_, Me, StackSends<Self, Own>>,
        _: Parked,
        _: Time,
    ) -> Result<Entered<()>, Fault> {
        Ok(Entered::Passed(()))
    }

    fn resume<Me, X: 'static, Own>(
        (): &mut Self::Values,
        at: usize,
        _: X,
        _: &Head,
        _: &mut Turn<'_, Me, StackSends<Self, Own>>,
        _: Parked,
        _: Time,
    ) -> Result<Entered<()>, Fault> {
        Ok(untaken(at))
    }

    fn outbound<Me, Own>(
        (): &mut Self::Values,
        _: usize,
        _: &Head,
        _: &mut Leaving<'_>,
        _: &mut Turn<'_, Me, StackSends<Self, Own>>,
        _: ParkedResponse,
        _: Time,
    ) -> Result<Returned, Fault> {
        Ok(Returned::Passed)
    }

    fn taker((): &Self::Values, _: usize) -> Taker {
        Taker::Pure
    }

    fn inbound_body((): &mut Self::Values, _: usize, _: &mut BodyPart<'_>) {}

    fn outbound_body((): &mut Self::Values, _: usize, _: &mut BodyPart<'_>) {}
}

/// Layers, each named with its place and the places in the stack's sends of its asks in and out, then the last layer
/// with its place, and the places of the last layer's asks of its handlers that the places left hold. The bounds that
/// chain each layer's `In` to the `Out` before it are gathered one layer at a time, carrying what is attached so far,
/// and each layer keeps the `In` it takes.
macro_rules! layered {
    ($( [$($layer:ident $at:tt $in:tt $out:tt)+] $last:ident $place:tt [$($handler:tt)*]; )*) => {$(
        layered!(@chain [] [()] [] [$($layer $at $in $out)+] $last $place [$($handler)*]);
    )*};
    (
        @chain [$($bounds:tt)*] [$attached:ty] [$($walked:tt)*] [$layer:ident $at:tt $in:tt $out:tt $($rest:tt)*]
        $last:ident $place:tt $handlers:tt
    ) => {
        layered!(
            @chain
            [$($bounds)* $layer: LayerForm + 'static, <$layer as LayerForm>::Form: Walks<$attached>,]
            [<<$layer as LayerForm>::Form as Walks<$attached>>::Out]
            [$($walked)* [$layer $at $in $out [$attached]]]
            [$($rest)*]
            $last
            $place
            $handlers
        );
    };
    (
        @chain [$($bounds:tt)*] [$attached:ty] [$([$layer:ident $at:tt $in:tt $out:tt [$taken:ty]])+] []
        $last:ident $place:tt [$($handler:tt)*]
    ) => {
        impl<$($layer,)+ $last> LayerList for ($($layer,)+ $last,)
        where
            $($bounds)*
            $last: LayerForm + 'static,
            <$last as LayerForm>::Form: LastLayer<$attached>,
        {
            type Forms = ($(<$layer as LayerForm>::Form,)+ <$last as LayerForm>::Form,);
            type Values = ($(<$layer as LayerForm>::Form,)+);
            type Last = <$last as LayerForm>::Form;
            type Attached = $attached;
            type Asked = (
                $(
                    <<$layer as LayerForm>::Form as Walks<$taken>>::Inbound,
                    <<$layer as LayerForm>::Form as Formed>::Outbound,
                )+
                $(<<<$last as LayerForm>::Form as LastLayer<$attached>>::Asked as Slot<At<$handler>>>::Item,)*
            );
            const LAYERS: usize = $place;

            fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
                Ok((
                    $(<<$layer as LayerForm>::Form as Formed>::found(turn)?,)+
                    <<$last as LayerForm>::Form as LastLayer<$attached>>::found(turn)?,
                ))
            }

            fn made(forms: &Self::Forms) -> Self::Values {
                ($(forms.$at.clone(),)+)
            }

            fn last(forms: &Self::Forms) -> &Self::Last {
                &forms.$place
            }

            fn inbound<Me, Own>(
                values: &mut Self::Values,
                head: &Head,
                turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
                parked: Parked,
                due: Time,
            ) -> Result<Entered<$attached>, Fault> {
                let attached = ();

                layered!(@walk values head turn parked due attached [$([$layer $at $in $out [$taken]])+])
            }

            #[allow(unused_variables, reason = "a list of one layer resumes at its last, and walks no layer more")]
            fn resume<Me, X: 'static, Own>(
                values: &mut Self::Values,
                at: usize,
                attached: X,
                head: &Head,
                turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
                parked: Parked,
                due: Time,
            ) -> Result<Entered<$attached>, Fault> {
                layered!(@resume values at head turn parked due attached [$([$layer $at $in $out [$taken]])+]);

                Ok(untaken(at))
            }

            fn outbound<Me, Own>(
                values: &mut Self::Values,
                at: usize,
                head: &Head,
                leaving: &mut Leaving<'_>,
                turn: &mut Turn<'_, Me, StackSends<Self, Own>>,
                parked: ParkedResponse,
                due: Time,
            ) -> Result<Returned, Fault> {
                $(
                    if at == $at {
                        let ask: Ask<At<$out>, ParkedResponse> = Ask::new(parked.at($at), due);
                        return <<$layer as LayerForm>::Form as Formed>::outbound(
                            &mut values.$at,
                            head,
                            leaving,
                            turn,
                            ask,
                        );
                    }
                )+

                Ok(Returned::Passed)
            }

            fn taker(values: &Self::Values, at: usize) -> Taker {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::taker(&values.$at),)+
                    _ => Taker::Pure,
                }
            }

            fn inbound_body(values: &mut Self::Values, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::inbound_body(&mut values.$at, part),)+
                    _ => {}
                }
            }

            fn outbound_body(values: &mut Self::Values, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => <<$layer as LayerForm>::Form as Formed>::outbound_body(&mut values.$at, part),)+
                    _ => {}
                }
            }
        }
    };
    (@walk $values:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident []) => {
        Ok(Entered::Passed($attached))
    };
    (
        @walk $values:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident
        [[$layer:ident $at:tt $in:tt $out:tt [$taken:ty]] $($rest:tt)*]
    ) => {{
        let ask: Ask<At<$in>, Parked> = Ask::new($parked.at($at), $due);
        let walked =
            <<$layer as LayerForm>::Form as Walks<$taken>>::inbound(&mut $values.$at, $head, $attached, $turn, ask)?;
        let $attached = match walked {
            Walked::Passed(attached) => attached,
            Walked::Answered(response) => return Ok(Entered::Answered { at: $at, response }),
            Walked::Parked => return Ok(Entered::Parked { at: $at }),
        };

        layered!(@walk $values $head $turn $parked $due $attached [$($rest)*])
    }};
    (@resume $values:ident $from:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident []) => {};
    (
        @resume $values:ident $from:ident $head:ident $turn:ident $parked:ident $due:ident $attached:ident
        [[$layer:ident $at:tt $in:tt $out:tt [$taken:ty]] $($rest:tt)*]
    ) => {
        if $from == $at {
            let taken: Option<<<$layer as LayerForm>::Form as Walks<$taken>>::Out> = as_taken($attached);
            let Some(taken) = taken else {
                return Ok(untaken($at));
            };

            return layered!(@walk $values $head $turn $parked $due taken [$($rest)*]);
        }

        layered!(@resume $values $from $head $turn $parked $due $attached [$($rest)*]);
    };
}

layered! {
    [L0 0 9 10] H 1 [0 1 2 3 4 5 6 7 8 9];
    [L0 0 9 10 L1 1 11 12] H 2 [0 1 2 3 4 5 6 7];
    [L0 0 9 10 L1 1 11 12 L2 2 13 14] H 3 [0 1 2 3 4 5];
    [L0 0 9 10 L1 1 11 12 L2 2 13 14 L3 3 15 16] H 4 [0 1 2 3];
    [L0 0 9 10 L1 1 11 12 L2 2 13 14 L3 3 15 16 L4 4 17 18] H 5 [0 1];
}
