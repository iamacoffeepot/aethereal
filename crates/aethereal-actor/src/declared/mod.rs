//! What an actor type declares: what it handles, and what each of its handlers may send (ADR-0013).
//!
//! An author writes the declaration as types: one [`Handles`] impl for each
//! kind an actor handles, whose `Sends` lists what its handler may send, and
//! the handled kinds as a tuple on the [`Actor`](crate::Actor). The typed turn
//! a handler is given offers only those sends, and a watch only to a handler
//! that lists [`Watches`], so the declaration is the handler's own signature.
//! An actor that declares a watch and handles no ending does not build. The engine reads the same declaration as the plain
//! data here, made from those types when the program is built.

mod handles;
mod protocols;
mod sends;
#[cfg(test)]
mod tests;

pub use handles::{Handled, HandledList, Handles, Others};
pub use protocols::{Answering, CarriedBy, Carries, Conforms, Covers, Listed, Protocol};
pub use sends::{Allows, At, Kinds, Marker, Sends, TakenBy, ToAny, ToSender, ToType, Watches};

use crate::Actor;
use aethereal_data::{ActorKey, Keyed, KindId, KindName};
use core::marker::PhantomData;

/// What an actor type declares, as the engine reads it: its name, what each of its turns may send, and what it
/// handles (ADR-0013).
#[derive(Debug, PartialEq, Eq)]
pub struct Declared {
    name: KindName,
    /// The type's id, made from its name and layout when the program is built: what the engine knows its kind by.
    id: KindId,
    /// Whether it has one actor or many, each under its own key.
    keyed: Keyed,
    /// What its `begin` may send.
    begins: &'static [Sending],
    /// Each thing it handles, and what its handler may send.
    handlers: &'static [Handler],
    /// What its closing turn may send.
    ends: &'static [Sending],
}

impl Declared {
    /// The declaration of the type named `name`, of id `id`, keyed as `keyed`, whose `begin` may send `begins`,
    /// which handles `handlers`, and whose closing turn may send `ends`.
    #[must_use]
    pub const fn new(
        name: KindName,
        id: KindId,
        keyed: Keyed,
        begins: &'static [Sending],
        handlers: &'static [Handler],
        ends: &'static [Sending],
    ) -> Self {
        Self { name, id, keyed, begins, handlers, ends }
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

    /// Whether it has one actor or many, each under its own key.
    #[must_use]
    pub const fn keyed(&self) -> Keyed {
        self.keyed
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

    /// Whether the type lists every one of `kinds`: the kinds of a protocol it takes (ADR-0016).
    #[must_use]
    pub fn lists_all(&self, kinds: &[KindId]) -> bool {
        kinds.iter().all(|kind| self.lists(*kind))
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

    /// Whether the type's handler of each kind `answering` names declares a reply of each kind it is answered with:
    /// what conforming to a named protocol asks of it (ADR-0016).
    #[must_use]
    pub const fn answers(&self, answering: &[Answering]) -> bool {
        let mut index = 0;
        while index < answering.len() {
            let answered = &answering[index];
            let mut reply = 0;
            while reply < answered.answers.len() {
                if !self.replies(answered.kind, answered.answers[reply]) {
                    return false;
                }
                reply += 1;
            }
            index += 1;
        }

        true
    }

    /// Whether the type's handler of `kind` declares a reply of kind `reply`.
    const fn replies(&self, kind: KindId, reply: KindId) -> bool {
        let mut index = 0;
        while index < self.handlers.len() {
            let handler = &self.handlers[index];
            if let Role::Kind(handled) = handler.role
                && handled.same(kind)
            {
                return replies_in(handler.sends, reply);
            }
            index += 1;
        }

        false
    }
}

/// Whether `sendings` declare a reply of kind `reply`.
const fn replies_in(sendings: &[Sending], reply: KindId) -> bool {
    let mut index = 0;
    while index < sendings.len() {
        let sending = &sendings[index];
        if matches!(sending.to, Target::Sender) && sending.kind.same(reply) {
            return true;
        }
        index += 1;
    }

    false
}

/// The declaration of `A`, as a constant.
struct Of<A>(PhantomData<A>);

impl<A: Actor> Of<A> {
    const DECLARED: &'static Declared = &Declared::new(
        A::NAME,
        KindId::of::<A>(),
        <A::Key as ActorKey>::KEYED,
        <A::BeginSends as Sends>::SENDINGS,
        <A::Handles as HandledList<A>>::HANDLERS,
        <A::EndSends as Sends>::SENDINGS,
    )
    .checked()
    .taking_back(Self::TAKES_BACK);

    /// Whether `A` takes whatever may come back from the sends of its `begin`, each handler, and its closing turn.
    const TAKES_BACK: bool = <A::BeginSends as TakenBy<A>>::TAKEN
        && <A::Handles as HandledList<A>>::TAKES_BACK
        && <A::EndSends as TakenBy<A>>::TAKEN;
}

impl Declared {
    /// It, when its type takes whatever may come back from each of its sends, as `takes` says (ADR-0016).
    ///
    /// # Panics
    ///
    /// Panics, as the type's declaration is made when the program is built, stopping the build, when it does not.
    const fn taking_back(self, takes: bool) -> Self {
        assert!(takes, "false declaration: a type sends mail whose answer it does not take");

        self
    }
}

/// Whether the declaration of `A` takes whatever may come back from its sends, read without making it: what makes
/// it fail to build when it does not.
#[cfg(test)]
pub const fn takes_back_of<A: Actor>() -> bool {
    Of::<A>::TAKES_BACK
}

/// What may come back to a sender from one send it declares (ADR-0016).
///
/// A type takes whatever may come back from each of its sends, or its
/// declaration does not build: an asker that takes no answer, asking an actor
/// that replies, does not.
///
/// ```compile_fail
/// use aethereal_actor::{Actor, Beginning, Declared, Ending, Fault, Handles, ToSender, ToType, Turn};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct Ask;
///
/// impl Kind for Ask {
///     const NAME: KindName = KindName::new("aether.example.ask");
/// }
///
/// #[derive(Plain)]
/// struct Answer;
///
/// impl Kind for Answer {
///     const NAME: KindName = KindName::new("aether.example.answer");
/// }
///
/// /// An actor that answers each question it is asked.
/// #[derive(Plain)]
/// struct Answerer;
///
/// impl Kind for Answerer {
///     const NAME: KindName = KindName::new("aether.example.answerer");
/// }
///
/// impl Actor for Answerer {
///     type Config = Answerer;
///     type Key = ();
///     type Handles = (Ask,);
///     type BeginSends = ();
///     type EndSends = ();
///
///     fn begin(answerer: Answerer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
///         Ok(answerer)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Ask> for Answerer {
///     type Sends = (ToSender<Answer>,);
///
///     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Answer)?)
///     }
/// }
///
/// /// An actor that asks, and takes no answer.
/// #[derive(Plain)]
/// struct Asker;
///
/// impl Kind for Asker {
///     const NAME: KindName = KindName::new("aether.example.asker");
/// }
///
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (ToType<Answerer, Ask>,);
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn main() {
///     let _ = Declared::of::<Asker>();
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// Nothing: a reply, a watch, or a send to a protocol of tells.
    Nothing,
    /// The replies among these, the sends of the receiver's handler of the kind sent: a send to an actor of a type.
    Replies(&'static [Sending]),
    /// What the kind sent is answered with, among a named protocol's answers.
    Answers(KindId, &'static [Answering]),
}

/// Whether an actor that handles `handlers` takes whatever may come back from a send that declares `back`: it lists
/// each kind, or it falls back (ADR-0016). Mail of a kind it does not take would reach it, and nothing could handle it.
const fn takes_back(handlers: &[Handler], back: Back) -> bool {
    match back {
        Back::Nothing => true,
        Back::Replies(sendings) => takes_replies(handlers, sendings),
        Back::Answers(kind, answering) => takes_answers(handlers, kind, answering),
    }
}

/// Whether an actor that handles `handlers` takes each reply among `sendings`.
const fn takes_replies(handlers: &[Handler], sendings: &[Sending]) -> bool {
    let mut index = 0;
    while index < sendings.len() {
        let sending = &sendings[index];
        let is_reply = matches!(sending.to, Target::Sender);
        if is_reply && !takes_kind(handlers, sending.kind) {
            return false;
        }
        index += 1;
    }

    true
}

/// Whether an actor that handles `handlers` takes each kind `kind` is answered with, as `answering` lists them.
const fn takes_answers(handlers: &[Handler], kind: KindId, answering: &[Answering]) -> bool {
    let mut index = 0;
    while index < answering.len() {
        let answered = &answering[index];
        if answered.kind.same(kind) {
            return takes_all(handlers, answered.answers);
        }
        index += 1;
    }

    true
}

/// Whether an actor that handles `handlers` takes mail of each of `kinds`.
const fn takes_all(handlers: &[Handler], kinds: &[KindId]) -> bool {
    let mut index = 0;
    while index < kinds.len() {
        if !takes_kind(handlers, kinds[index]) {
            return false;
        }
        index += 1;
    }

    true
}

/// Whether an actor that handles `handlers` takes mail of `kind`: it lists the kind, or it falls back.
const fn takes_kind(handlers: &[Handler], kind: KindId) -> bool {
    let mut index = 0;
    while index < handlers.len() {
        let taken = match handlers[index].role {
            Role::Kind(handled) => handled.same(kind),
            Role::Others => true,
            Role::Ended => false,
        };
        if taken {
            return true;
        }
        index += 1;
    }

    false
}

impl Declared {
    /// It, when what it declares holds together: a type that declares a watch handles endings, as the ending of what
    /// it watches is handed to it, and each send it declares to a protocol is of one of the protocol's kinds, as
    /// nothing that takes the protocol need take another (ADR-0016).
    ///
    /// # Panics
    ///
    /// Panics, as the type's declaration is made when the program is built, stopping the build, when it does not.
    const fn checked(self) -> Self {
        let mut watches = declares_watch(self.begins) || declares_watch(self.ends);
        let mut sends_in_protocols = in_protocols(self.begins) && in_protocols(self.ends);
        let mut handles_endings = false;
        let mut index = 0;
        while index < self.handlers.len() {
            let handler = &self.handlers[index];
            watches = watches || declares_watch(handler.sends);
            sends_in_protocols = sends_in_protocols && in_protocols(handler.sends);
            handles_endings = handles_endings || matches!(handler.role, Role::Ended);
            index += 1;
        }
        let holds = !watches || handles_endings;
        assert!(holds, "false declaration: a type declares a watch, and does not handle endings");
        assert!(
            sends_in_protocols,
            "false declaration: a type declares a send to a protocol of a kind the protocol does not hold"
        );

        self
    }
}

/// Whether `sendings` declare a watch.
const fn declares_watch(sendings: &[Sending]) -> bool {
    let mut index = 0;
    while index < sendings.len() {
        if matches!(sendings[index].to, Target::Watched) {
            return true;
        }
        index += 1;
    }

    false
}

/// Whether each of `sendings` to a protocol is of one of its kinds.
const fn in_protocols(sendings: &[Sending]) -> bool {
    let mut index = 0;
    while index < sendings.len() {
        let sending = &sendings[index];
        if let Target::Handling(kinds) = sending.to
            && !holds_kind(kinds, sending.kind)
        {
            return false;
        }
        index += 1;
    }

    true
}

/// Whether `kinds` holds `kind`.
const fn holds_kind(kinds: &[KindId], kind: KindId) -> bool {
    let mut index = 0;
    while index < kinds.len() {
        if kinds[index].same(kind) {
            return true;
        }
        index += 1;
    }

    false
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
    /// No actor: the handler may watch one, which is no send.
    Watched,
}
