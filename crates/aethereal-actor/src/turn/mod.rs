//! A turn: what an actor asks of the engine while it handles one piece of mail (ADR-0009).

mod refused;
#[cfg(test)]
mod tests;

pub use refused::Refused;

use crate::{Actor, Actors, ActorsWriter, Code, Effect, ErasedActorRef, Runs, Table, To, ToWithoutMail, What};
use aethereal_data::{ActorId, ActorPath, Encode, EncodeError, Kind, KindId, PathSegment, PathTooDeep, Writer};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// What one turn has asked for: its effects in order, and the children it stood up (ADR-0009).
///
/// The engine makes one for a turn and takes it back when the turn returns.
/// Nothing in it has happened yet: the engine alone applies effects.
#[derive(Default)]
pub struct Asked {
    effects: Vec<Effect>,
    stood: Vec<Stood>,
}

impl Asked {
    /// Nothing asked for yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The effects asked for so far, in the order they were asked for.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// The effects, and the children stood up, each child in the order its `begin` finished.
    #[must_use]
    pub fn into_parts(self) -> (Vec<Effect>, Vec<Stood>) {
        (self.effects, self.stood)
    }

    /// Whether a child stood up in this turn has the id `id`.
    fn has_stood(&self, id: ActorId) -> bool {
        self.stood.iter().any(|child| child.id == id)
    }
}

/// A child stood up in a turn and held aside until the engine applies its spawn (ADR-0009).
///
/// Until then it exists for no one but its parent.
pub struct Stood {
    id: ActorId,
    path: ActorPath,
    actor: Box<dyn Runs>,
}

impl Stood {
    /// The child's id.
    #[must_use]
    pub const fn id(&self) -> ActorId {
        self.id
    }

    /// Where the child stands.
    #[must_use]
    pub const fn path(&self) -> &ActorPath {
        &self.path
    }

    /// The child itself, for the engine to run.
    #[must_use]
    pub fn into_actor(self) -> Box<dyn Runs> {
        self.actor
    }
}

/// Whose turn it is, and what every kind of turn works with.
struct Seat<'a> {
    id: ActorId,
    path: ActorPath,
    table: &'a dyn Table,
    asked: &'a mut Asked,
}

impl Seat<'_> {
    /// Adds the effect by which this actor asks for `what`.
    fn ask(&mut self, what: What) {
        self.asked.effects.push(Effect::new(self.id, what));
    }

    /// A reference to this actor.
    const fn me(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.id)
    }

    /// A reference to the live actor `path` names.
    fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.table.resolve(path).map(ErasedActorRef::new)
    }

    /// Adds the effect that watches the actor `of` refers to.
    fn watch(&mut self, of: ErasedActorRef) {
        self.ask(What::Watch { of });
    }

    /// Encodes `mail` and adds the effect that sends it to `to`.
    fn send<K: Kind + Encode<Actors>>(&mut self, to: To, mail: &K) -> Result<(), EncodeError> {
        let mut out = Writer::<Actors>::new(ActorsWriter::new());
        mail.encode(&mut out)?;
        self.ask(What::Send { to, kind: KindId::of::<K>(), bytes: out.into_bytes() });

        Ok(())
    }

    /// Stands up a child of type `C` under `key`: adds its spawn effect, then runs its `begin` here.
    ///
    /// A key is free when no actor has been made at the child's path and this turn has stood none up there. An
    /// id the table holds under another path is not refused: that is two paths with one id, and the engine
    /// stops when it folds the spawn (ADR-0008).
    fn spawn<C: Actor + 'static>(&mut self, key: &C::Key, config: C::Config) -> Result<ErasedActorRef, Refused> {
        let segment = PathSegment::new(C::NAME, key);
        let path = self.path.child(&segment).map_err(|PathTooDeep| Refused::TooDeep)?;
        let id = self.table.child(self.id, &segment);
        let is_made = self.table.path(id).as_ref() == Some(&path);
        let is_taken = is_made || self.asked.has_stood(id);
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        let from = Code::Linked(C::NAME);
        let spawn = match segment.key() {
            None => What::Spawn { from },
            Some(key) => What::SpawnInstanced { from, key: key.clone() },
        };
        self.ask(spawn);

        let stood_before = self.asked.stood.len();
        let seat = Seat { id, path: path.clone(), table: self.table, asked: &mut *self.asked };
        match C::begin(config, &mut Beginning { seat }) {
            Ok(actor) => {
                self.asked.stood.push(Stood { id, path, actor: Box::new(actor) });

                Ok(ErasedActorRef::new(id))
            }
            Err(fault) => {
                self.asked.effects.push(Effect::new(id, What::Faulted));
                self.asked.stood.truncate(stood_before);

                Err(Refused::Began(fault))
            }
        }
    }
}

/// What an actor asks of the engine while it handles one piece of mail (ADR-0009).
///
/// Each ask adds an effect to the turn's list, and nothing asked for happens
/// until the turn has returned `Ok`. Each read answers from the table of
/// actors as it stood when the turn's step began.
pub struct Turn<'a> {
    seat: Seat<'a>,
}

impl<'a> Turn<'a> {
    /// The turn of the actor `id` stands for, at `path`, reading `table` and adding to `asked`.
    ///
    /// For the engine. `table` answers for the step's start and for what this actor has spawned since.
    #[must_use]
    pub const fn new(id: ActorId, path: ActorPath, table: &'a dyn Table, asked: &'a mut Asked) -> Self {
        Self { seat: Seat { id, path, table, asked } }
    }

    /// Watches the actor `of` refers to: when it ends, this actor's [`Actor::ended`] runs for it, once.
    ///
    /// Watching an actor already watched changes nothing.
    pub fn watch(&mut self, of: ErasedActorRef) {
        self.seat.watch(of);
    }

    /// Sends `mail` to `to`. It is delivered after the turn, in a later step.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: To, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(to, mail)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    ///
    /// The child takes its own mail. It is live once the engine applies this turn's effects.
    pub fn spawn<C: Actor + 'static>(&mut self, key: &C::Key, config: C::Config) -> Result<ErasedActorRef, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Closes this actor. Its closing turn runs in the next step.
    pub fn close(&mut self) {
        self.seat.ask(What::Close);
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }
}

/// What an actor asks of the engine in its `begin`: a first turn, which offers less than a turn (ADR-0009).
///
/// It offers no close: an actor does not end in the turn that makes it. It is handling no mail, so it has no
/// sender to send to.
pub struct Beginning<'a> {
    seat: Seat<'a>,
}

impl<'a> Beginning<'a> {
    /// The `begin` of the root `id` stands for, at `path`, reading `table` and adding to `asked`.
    ///
    /// For the engine, which makes a root for the chassis. A child's is made by the turn that spawns it.
    #[must_use]
    pub const fn new(id: ActorId, path: ActorPath, table: &'a dyn Table, asked: &'a mut Asked) -> Self {
        Self { seat: Seat { id, path, table, asked } }
    }

    /// Sends `mail` to `to`. It goes out with the mail of the turn this `begin` runs in.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(to.into(), mail)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    pub fn spawn<C: Actor + 'static>(&mut self, key: &C::Key, config: C::Config) -> Result<ErasedActorRef, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Watches the actor `of` refers to: when it ends, this actor's [`Actor::ended`] runs for it, once.
    pub fn watch(&mut self, of: ErasedActorRef) {
        self.seat.watch(of);
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }
}

/// What an actor asks of the engine while it reacts to the ending of an actor it watches (ADR-0009).
///
/// It offers what a turn does. It is handling no mail, so it has no sender to send to.
pub struct Noticing<'a> {
    seat: Seat<'a>,
}

impl<'a> Noticing<'a> {
    /// The turn of the actor `id` stands for, at `path`, reacting to an ending, reading `table` and adding to
    /// `asked`.
    ///
    /// For the engine.
    #[must_use]
    pub const fn new(id: ActorId, path: ActorPath, table: &'a dyn Table, asked: &'a mut Asked) -> Self {
        Self { seat: Seat { id, path, table, asked } }
    }

    /// Sends `mail` to `to`. It is delivered after the turn, in a later step.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(to.into(), mail)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    pub fn spawn<C: Actor + 'static>(&mut self, key: &C::Key, config: C::Config) -> Result<ErasedActorRef, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Closes this actor. Its closing turn runs in the next step.
    pub fn close(&mut self) {
        self.seat.ask(What::Close);
    }

    /// Watches the actor `of` refers to: when it ends, this actor's [`Actor::ended`] runs for it, once.
    pub fn watch(&mut self, of: ErasedActorRef) {
        self.seat.watch(of);
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }
}

/// Why an actor's closing turn runs (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The actor asked to close.
    Closed,
    /// A turn of the actor's own returned a fault. Its state is whatever that turn left.
    Faulted,
}

/// What an actor asks of the engine in its closing turn (ADR-0009).
///
/// It offers no spawn and no close. It is handling no mail, so it has no sender to send to.
pub struct Ending<'a> {
    seat: Seat<'a>,
    why: Why,
}

impl<'a> Ending<'a> {
    /// The closing turn of the actor `id` stands for, at `path`, which runs because of `why`.
    ///
    /// For the engine.
    #[must_use]
    pub const fn new(id: ActorId, path: ActorPath, table: &'a dyn Table, asked: &'a mut Asked, why: Why) -> Self {
        Self { seat: Seat { id, path, table, asked }, why }
    }

    /// Why this closing turn runs.
    #[must_use]
    pub const fn why(&self) -> Why {
        self.why
    }

    /// Sends `mail` to `to`: the actor's last mail.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(to.into(), mail)
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }
}
