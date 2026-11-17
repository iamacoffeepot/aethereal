//! A stack's layers, listed as one tuple of types in the order a request passes them (ADR-0035): each pure layer, and
//! last the handler. The tuple says what each layer is held as, what the layers attach along it, and how the stack
//! walks a request, a response, and each part of a body through them.

use super::{BodyPart, Head, Layer, LayerForm, Leaving, Response, Step};
use aethereal_actor::{Actor, ActorRef, At, Beginning, ChildOf, Fault, Lists, PathTo, Within, path};
use aethereal_data::{Bound, Decode, Encode, Kind, KindName, Plain};

/// A stack's layers, as one tuple of types: its pure layers, each a [`Layer`] whose `In` is the `Out` of the one
/// before it, the first's `()`, and last its handler, an actor that is a singleton root, asked a
/// [`Request`](super::Request) of what the layers attached. Written for tuples of up to eight.
pub trait LayerList: 'static {
    /// The tuple of its layers' forms: what the stack holds to use them, written out by the compiler from the list.
    type Forms: Encode<Within> + Decode<Within> + Clone;
    /// The handler: the last layer, which answers.
    type Handler: Actor;
    /// What the pure layers attach to a request, which the handler is asked with.
    type Attached: Plain;
    /// How many pure layers stand before the handler.
    const PURE: usize;

    /// Each layer as the stack holds it: a pure layer made with `Default`, and the handler found at its typed path as
    /// `turn` sees the engine.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`], naming the handler's type, when it is not live.
    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault>;

    /// The handler, as `forms` hold it.
    fn handler(forms: &Self::Forms) -> ActorRef<Self::Handler>;

    /// Passes `head` in through each pure layer of `forms`, in order: what they attached, or the response of the one
    /// that answered.
    fn inbound(forms: &mut Self::Forms, head: &Head) -> Entered<Self::Attached>;

    /// Hands the pure layer at `at` of `forms` a response on its way out.
    fn outbound(forms: &mut Self::Forms, at: usize, head: &Head, leaving: &mut Leaving<'_>);

    /// Hands the pure layer at `at` of `forms` a part of the request's body.
    fn inbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>);

    /// Hands the pure layer at `at` of `forms` a part of the response's body.
    fn outbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>);
}

/// What became of a request on its way in through a stack's pure layers.
#[derive(Debug)]
pub enum Entered<A> {
    /// Every layer passed it, attaching `A`: it goes on to the handler.
    Passed(A),
    /// The layer at `at` answered it with `response`, which turns back through the layers before that one.
    Answered { at: usize, response: Response },
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

/// The stack's layers, `T`, read from the annotation, each built as what it is: a pure layer with `Default`, and the
/// handler found at its typed path with the begin's turn.
///
/// # Errors
///
/// The fault [`Unlayered`], naming the handler's type, when it is not live.
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

/// The handler `H` is not live, as the fault a stack's `begin` fails with.
fn unlayered<H: Kind>() -> Fault {
    Fault::new(&Unlayered { layer: H::NAME }).unwrap_or_else(Fault::from)
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
}

/// A handler alone, with no pure layer before it, is handed every request as it came, with nothing attached.
impl<H> LayerList for (H,)
where
    H: Actor + ChildOf + PathTo<Keys = ()>,
{
    type Forms = (<H as LayerForm>::Form,);
    type Handler = H;
    type Attached = ();
    const PURE: usize = 0;

    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
        let handler = turn.found(&path::<H>(())).ok_or_else(unlayered::<H>)?;

        Ok((handler,))
    }

    fn handler(forms: &Self::Forms) -> ActorRef<H> {
        forms.0
    }

    fn inbound(_: &mut Self::Forms, _: &Head) -> Entered<()> {
        Entered::Passed(())
    }

    fn outbound(_: &mut Self::Forms, _: usize, _: &Head, _: &mut Leaving<'_>) {}

    fn inbound_body(_: &mut Self::Forms, _: usize, _: &mut BodyPart<'_>) {}

    fn outbound_body(_: &mut Self::Forms, _: usize, _: &mut BodyPart<'_>) {}
}

/// Pure layers, each named with its place, then the handler with its place. The bounds that chain each layer's `In`
/// to the `Out` before it are gathered one layer at a time, carrying what is attached so far.
macro_rules! layered {
    ($( [$($pure:ident $at:tt)+] $handler:ident $last:tt; )*) => {$(
        layered!(@chain [] [()] [$($pure $at)+] [$($pure $at)+] $handler $last);
    )*};
    (@chain [$($bounds:tt)*] [$attached:ty] [$pure:ident $at:tt $($rest:tt)*] $all:tt $handler:ident $last:tt) => {
        layered!(
            @chain
            [$($bounds)* $pure: Layer<In = $attached> + LayerForm<Form = $pure> + Encode<Within> + Decode<Within>,]
            [<$pure as Layer>::Out]
            [$($rest)*]
            $all
            $handler
            $last
        );
    };
    (@chain [$($bounds:tt)*] [$attached:ty] [] [$($pure:ident $at:tt)+] $handler:ident $last:tt) => {
        impl<$($pure,)+ $handler> LayerList for ($($pure,)+ $handler,)
        where
            $($bounds)*
            $handler: Actor + ChildOf + PathTo<Keys = ()>,
        {
            type Forms = ($(<$pure as LayerForm>::Form,)+ <$handler as LayerForm>::Form,);
            type Handler = $handler;
            type Attached = $attached;
            const PURE: usize = $last;

            fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self::Forms, Fault> {
                let handler = turn.found(&path::<$handler>(())).ok_or_else(unlayered::<$handler>)?;

                Ok(($($pure::default(),)+ handler,))
            }

            fn handler(forms: &Self::Forms) -> ActorRef<$handler> {
                forms.$last
            }

            fn inbound(forms: &mut Self::Forms, head: &Head) -> Entered<$attached> {
                let attached = ();
                $(
                    let attached = match forms.$at.inbound(head, attached) {
                        Step::Pass(attached) => attached,
                        Step::Answer(response) => return Entered::Answered { at: $at, response },
                    };
                )+

                Entered::Passed(attached)
            }

            fn outbound(forms: &mut Self::Forms, at: usize, head: &Head, leaving: &mut Leaving<'_>) {
                match at {
                    $($at => forms.$at.outbound(head, leaving),)+
                    _ => {}
                }
            }

            fn inbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => forms.$at.inbound_body(part),)+
                    _ => {}
                }
            }

            fn outbound_body(forms: &mut Self::Forms, at: usize, part: &mut BodyPart<'_>) {
                match at {
                    $($at => forms.$at.outbound_body(part),)+
                    _ => {}
                }
            }
        }
    };
}

layered! {
    [L0 0] H 1;
    [L0 0 L1 1] H 2;
    [L0 0 L1 1 L2 2] H 3;
    [L0 0 L1 1 L2 2 L3 3] H 4;
    [L0 0 L1 1 L2 2 L3 3 L4 4] H 5;
    [L0 0 L1 1 L2 2 L3 3 L4 4 L5 5] H 6;
    [L0 0 L1 1 L2 2 L3 3 L4 4 L5 5 L6 6] H 7;
}
