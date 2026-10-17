//! What an actor type declares: what it handles, and what each of its handlers may send (ADR-0013).
//!
//! An author writes the declaration as types: one [`Handles`] impl for each
//! kind an actor handles, whose `Sends` lists what its handler may send, and
//! the handled kinds as a tuple on the [`Actor`](crate::Actor). The typed turn
//! a handler is given offers only those sends, so the declaration is the
//! handler's own signature. The engine reads the same declaration as the plain
//! data here, made from those types when the program is built.

mod handles;
mod sends;
#[cfg(test)]
mod tests;

pub use handles::{Handled, HandledList, Handles, Others};
pub use sends::{Allows, At, Kinds, Marker, Sends, ToAny, ToSender, ToType};

use crate::Actor;
use aethereal_data::{KindId, KindName};
use core::marker::PhantomData;

/// What an actor type declares, as the engine reads it: its name, what each of its turns may send, and what it
/// handles (ADR-0013).
#[derive(Debug, PartialEq, Eq)]
pub struct Declared {
    name: KindName,
    /// The type's id, made from its name and layout when the program is built: what the engine knows its kind by.
    id: KindId,
    /// What its `begin` may send.
    begins: &'static [Sending],
    /// Each thing it handles, and what its handler may send.
    handlers: &'static [Handler],
    /// What its closing turn may send.
    ends: &'static [Sending],
}

impl Declared {
    /// The declaration of the type named `name`, of id `id`, whose `begin` may send `begins`, which handles
    /// `handlers`, and whose closing turn may send `ends`.
    #[must_use]
    pub const fn new(
        name: KindName,
        id: KindId,
        begins: &'static [Sending],
        handlers: &'static [Handler],
        ends: &'static [Sending],
    ) -> Self {
        Self { name, id, begins, handlers, ends }
    }

    /// The declaration of the actor type `A`, made when the program is built.
    #[must_use]
    pub const fn of<A: Actor>() -> &'static Self {
        Of::<A>::DECLARED
    }

    /// The type's name: the kind of every actor of it, in its path.
    #[must_use]
    pub const fn name(&self) -> &KindName {
        &self.name
    }

    /// The type's id: the kind of every actor of it, as the engine knows it.
    #[must_use]
    pub const fn id(&self) -> KindId {
        self.id
    }

    /// What its `begin` may send.
    #[must_use]
    pub const fn begins(&self) -> &'static [Sending] {
        self.begins
    }

    /// Each thing it handles, and what its handler may send.
    #[must_use]
    pub const fn handlers(&self) -> &'static [Handler] {
        self.handlers
    }

    /// What its closing turn may send.
    #[must_use]
    pub const fn ends(&self) -> &'static [Sending] {
        self.ends
    }

    /// Whether the type lists `kind` among the kinds it handles. A fallback does not count.
    #[must_use]
    pub fn lists(&self, kind: KindId) -> bool {
        self.handles(Role::Kind(kind))
    }

    /// Whether the type takes mail of `kind`: it lists the kind, or it falls back.
    #[must_use]
    pub fn takes(&self, kind: KindId) -> bool {
        self.lists(kind) || self.handles(Role::Others)
    }

    /// Whether the type handles the ending of an actor it watches.
    #[must_use]
    pub fn handles_endings(&self) -> bool {
        self.handles(Role::Ended)
    }

    /// Whether one of the type's handlers is for `role`.
    fn handles(&self, role: Role) -> bool {
        self.handlers.iter().any(|handler| handler.role == role)
    }
}

/// The declaration of `A`, as a constant.
struct Of<A>(PhantomData<A>);

impl<A: Actor> Of<A> {
    const DECLARED: &'static Declared = &Declared::new(
        A::NAME,
        KindId::of::<A>(),
        <A::BeginSends as Sends>::SENDINGS,
        <A::Handles as HandledList<A>>::HANDLERS,
        <A::EndSends as Sends>::SENDINGS,
    );
}

/// One thing an actor type handles, and what its handler may send (ADR-0013).
#[derive(Debug, PartialEq, Eq)]
pub struct Handler {
    role: Role,
    sends: &'static [Sending],
}

impl Handler {
    /// The handler for `role`, which may send `sends`.
    #[must_use]
    pub const fn new(role: Role, sends: &'static [Sending]) -> Self {
        Self { role, sends }
    }

    /// What it handles.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// What it may send.
    #[must_use]
    pub const fn sends(&self) -> &'static [Sending] {
        self.sends
    }
}

/// One thing an actor type handles (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Mail of this kind.
    Kind(KindId),
    /// The ending of an actor it watches.
    Ended,
    /// Mail of any kind it does not list: its fallback.
    Others,
}

/// One send a handler may make: mail of a kind, to a target (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sending {
    kind: KindId,
    to: Target,
}

impl Sending {
    /// The send of mail of `kind` to `to`.
    #[must_use]
    pub const fn new(kind: KindId, to: Target) -> Self {
        Self { kind, to }
    }

    /// The kind of mail sent.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// Where it may go.
    #[must_use]
    pub const fn to(&self) -> Target {
        self.to
    }
}

/// Where a declared send may go (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An actor of the type of this name, and of this id: what a send is checked by, a name being slower to compare.
    Type(&'static KindName, KindId),
    /// The source of the item being handled.
    Sender,
    /// Any actor whose type lists every one of these kinds: what a reference that names a protocol, and not a type,
    /// reaches.
    Handling(&'static [KindId]),
}
