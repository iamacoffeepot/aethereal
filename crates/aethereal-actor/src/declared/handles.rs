//! What an actor handles: one handler for each thing it lists, and the engine's call to the right one (ADR-0013).

use super::{BackTo, Handler, Role, Sends};
use crate::{Actor, Actors, Ended, Fault, Item, Mail, Turn, Turning};
use aethereal_data::{Decode, Kind, KindId};

/// The actor handles `H`: a kind of mail, the ending of an actor it watches ([`Ended`]), or any mail of a kind it does
/// not list ([`Others`]) (ADR-0013).
///
/// `Sends` lists what the handler may send, and the turn it is given offers
/// those sends and no other, so what it declares is what it can do.
pub trait Handles<H: Handled>: Actor {
    /// What the handler may send: a tuple of markers, whatever comes back from which the actor handles.
    type Sends: Sends + BackTo<Self>;

    /// Handles one item: a turn of its own.
    fn handle(&mut self, item: H::Item<'_>, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault>;
}

/// Mail of any kind its receiver does not list: the receiver's fallback, which is handed the mail as it came
/// (ADR-0013).
pub struct Others;

/// Something an actor can handle: a kind of mail, [`Ended`], or [`Others`] (ADR-0013).
pub trait Handled: 'static {
    /// What it is, as the engine reads it.
    const ROLE: Role;
    /// What its handler is handed.
    type Item<'a>;

    /// What the handler is handed for `item`, which is of this role.
    fn take(item: Item<'_>) -> Result<Self::Item<'_>, Fault>;
}

impl<K: Kind + Decode<Actors> + 'static> Handled for K {
    const ROLE: Role = Role::Kind(KindId::of::<K>());
    type Item<'a> = K;

    fn take(item: Item<'_>) -> Result<K, Fault> {
        let Item::Mail(mail) = item else {
            unreachable!("only mail of its kind is handed to a kind's handler");
        };

        Ok(mail.contents()?)
    }
}

impl Handled for Ended {
    const ROLE: Role = Role::Ended;
    type Item<'a> = Self;

    fn take(item: Item<'_>) -> Result<Self, Fault> {
        let Item::Ended(ended) = item else {
            unreachable!("only an ending is handed to the handler of endings");
        };

        Ok(ended)
    }
}

impl Handled for Others {
    const ROLE: Role = Role::Others;
    type Item<'a> = Mail<'a>;

    fn take(item: Item<'_>) -> Result<Mail<'_>, Fault> {
        let Item::Mail(mail) = item else {
            unreachable!("only mail is handed to a fallback");
        };

        Ok(mail)
    }
}

/// Everything an actor of type `A` handles: a tuple of [`Handled`], each with its [`Handles`] impl (ADR-0013).
pub trait HandledList<A: Actor>: 'static {
    /// Each thing handled, and what its handler may send, as the engine reads them.
    const HANDLERS: &'static [Handler];

    /// Hands `item` to the handler for it: the one for its kind or for endings, else the fallback for mail.
    ///
    /// # Panics
    ///
    /// Panics when `A` has no handler for `item`: its sender declared a send that `A` does not take (ADR-0013).
    fn dispatch(actor: &mut A, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;
}

/// What `item` is, as its receiver's handlers are listed.
fn role(item: &Item<'_>) -> Role {
    match item {
        Item::Mail(mail) => Role::Kind(mail.kind()),
        Item::Ended(_) => Role::Ended,
        Item::Answer { mail, .. } => Role::Answer(mail.kind()),
    }
}

/// Stops the engine: an actor of type `A` was handed an item it has no handler for.
fn unhandled<A: Actor>(role: Role) -> ! {
    panic!("false declaration: {role:?} was sent to an actor of type {}, which does not take it", A::NAME);
}

macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<A: Actor $(+ Handles<$name>)*, $($name: Handled),*> HandledList<A> for ($($name,)*) {
            const HANDLERS: &'static [Handler] =
                &[$(Handler::new($name::ROLE, <<A as Handles<$name>>::Sends as Sends>::SENDINGS)),*];

            fn dispatch(actor: &mut A, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
                let role = role(&item);
                $(
                    if $name::ROLE == role {
                        let mut turn = turning.declaring::<A, <A as Handles<$name>>::Sends>(role);
                        return <A as Handles<$name>>::handle(actor, $name::take(item)?, &mut turn);
                    }
                )*
                let is_mail = matches!(item, Item::Mail(_));
                $(
                    let falls_back = $name::ROLE == Role::Others;
                    if is_mail && falls_back {
                        let mut turn = turning.declaring::<A, <A as Handles<$name>>::Sends>(Role::Others);
                        return <A as Handles<$name>>::handle(actor, $name::take(item)?, &mut turn);
                    }
                )*

                unhandled::<A>(role)
            }
        }
    )*};
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
    [B C D E F G H I J K]
    [B C D E F G H I J K L]
    [B C D E F G H I J K L M]
}

impl<A: Actor> HandledList<A> for () {
    const HANDLERS: &'static [Handler] = &[];

    fn dispatch(_: &mut A, item: Item<'_>, _: Turning<'_>) -> Result<(), Fault> {
        unhandled::<A>(role(&item))
    }
}
