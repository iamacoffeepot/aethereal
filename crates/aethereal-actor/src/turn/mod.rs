//! A turn: what an actor asks of the engine while it handles one item (ADR-0009).

mod refused;
#[cfg(test)]
mod tests;

pub use refused::Refused;

use crate::{
    Actor, Actors, ActorsWriter, Arena, Code, Effect, ErasedActorRef, Level, MailId, Runs, Table, To, ToWithoutMail,
    What,
};
use aethereal_data::{ActorId, ActorPath, Encode, EncodeError, Kind, KindId, PathSegment, PathTooDeep, Writer};
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

/// What one turn has asked for: its effects in order, and the children it stood up (ADR-0009).
///
/// The engine makes one for a turn and takes it back when the turn returns.
/// Nothing in it has happened yet: the engine alone applies effects.
#[derive(Default)]
pub struct Asked {
    effects: Vec<Effect>,
    stood: Vec<Stood>,
    /// The bits of the ids of the children in `stood`, to find whether one stands at a key in a lookup.
    stood_ids: BTreeSet<u64>,
    /// Where the bytes of the mail the turn sends are written.
    arena: Arena,
    /// Where this turn's effects begin in `effects`: what is before them is earlier turns'.
    start: usize,
}

impl Asked {
    /// Nothing asked for yet, with mail written to an arena of its own.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Nothing asked for yet, with mail written to `arena`, after what it already holds.
    #[must_use]
    pub fn writing(arena: Arena) -> Self {
        Self::continuing(Vec::new(), arena)
    }

    /// Nothing asked for yet, with effects added after `effects`, the effects of earlier turns kept in one list,
    /// and mail written to `arena`, after what it already holds.
    #[must_use]
    pub fn continuing(effects: Vec<Effect>, arena: Arena) -> Self {
        let start = effects.len();

        Self { effects, stood: Vec::new(), stood_ids: BTreeSet::new(), arena, start }
    }

    /// The arena the turn's mail was written to.
    #[must_use]
    pub const fn arena(&self) -> &Arena {
        &self.arena
    }

    /// The effects asked for so far, in the order they were asked for.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects[self.start..]
    }

    /// The effects, earlier turns' first when the list was continued, the children stood up, each child in the order
    /// its `begin` finished, and the arena the mail was written to.
    #[must_use]
    pub fn into_parts(self) -> (Vec<Effect>, Vec<Stood>, Arena) {
        (self.effects, self.stood, self.arena)
    }

    /// Whether a child stood up in this turn has the id `id`.
    fn has_stood(&self, id: ActorId) -> bool {
        self.stood_ids.contains(&id.bits())
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
    /// How many pieces of mail this actor has sent, this turn's included.
    sent: u64,
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

    /// Adds the effect that logs `text` at `level`.
    fn log(&mut self, level: Level, text: impl Into<String>) {
        self.ask(What::Log { level, text: text.into() });
    }

    /// Adds the effect that watches the actor `of` refers to.
    fn watch(&mut self, of: ErasedActorRef) {
        self.ask(What::Watch { of });
    }

    /// Encodes `mail` and adds the effect that sends it to the actor `to` names.
    ///
    /// Nothing is sent when `to` names no actor this turn can see: a parent no longer live, or a root's parent.
    /// Whether the actor it names is live when the mail would be handed to it is the engine's to see, and mail to
    /// one that is not is dropped there.
    fn send<K: Kind + Encode<Actors>>(&mut self, to: &To, mail: &K) -> Result<(), EncodeError> {
        let Some(to) = self.receiver(to) else {
            return Ok(());
        };
        let bytes = self.asked.arena.write(|bytes| {
            let mut out = Writer::<Actors>::appending(bytes, ActorsWriter::new());
            let written = mail.encode(&mut out);

            (out.into_bytes(), written)
        })?;
        self.sent += 1;
        let id = MailId { sender: self.id, count: self.sent };
        self.ask(What::Send { to, id, kind: KindId::of::<K>(), bytes });

        Ok(())
    }

    /// The id of the actor `to` names, beside this one: none for a parent this turn cannot see.
    fn receiver(&self, to: &To) -> Option<ActorId> {
        match to {
            To::Parent => self.path.parent().and_then(|parent| self.table.resolve(&parent)),
            To::Child(kind) => Some(self.table.child(self.id, &PathSegment::new(kind.clone(), &()))),
            To::ChildInstanced(kind, key) => {
                Some(self.table.child(self.id, &PathSegment::instanced(kind.clone(), key.clone())))
            }
            To::Held(reference) => Some(reference.id()),
            // A turn puts the source of its item in its place before it asks.
            To::Sender => None,
        }
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
        let seat = Seat { id, path: path.clone(), table: self.table, asked: &mut *self.asked, sent: 0 };
        match C::begin(config, &mut Beginning { seat }) {
            Ok(actor) => {
                self.asked.stood_ids.insert(id.bits());
                self.asked.stood.push(Stood { id, path, actor: Box::new(actor) });

                Ok(ErasedActorRef::new(id))
            }
            Err(fault) => {
                self.asked.effects.push(Effect::new(id, What::Faulted));
                for child in self.asked.stood.drain(stood_before..) {
                    self.asked.stood_ids.remove(&child.id.bits());
                }

                Err(Refused::Began(fault))
            }
        }
    }
}

/// What an actor asks of the engine while it handles one item (ADR-0009).
///
/// Each ask adds an effect to the turn's list, and nothing asked for happens
/// until the turn has returned `Ok`. Each read answers from the table of
/// actors as it stood when the turn's step began.
pub struct Turn<'a> {
    seat: Seat<'a>,
    /// The actor the item being handled comes from: who [`To::Sender`] names.
    source: ActorId,
}

impl<'a> Turn<'a> {
    /// The turn of the actor `id` stands for, at `path`, reading `table` and adding to `asked`, which handles an
    /// item from `source` and has sent `sent` pieces of mail before it.
    ///
    /// For the engine. `table` answers for the step's start and for what this actor has spawned since.
    #[must_use]
    pub const fn new(
        id: ActorId,
        path: ActorPath,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        sent: u64,
        source: ActorId,
    ) -> Self {
        Self { seat: Seat { id, path, table, asked, sent }, source }
    }

    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    ///
    /// Watching an actor already watched changes nothing.
    pub fn watch(&mut self, of: ErasedActorRef) {
        self.seat.watch(of);
    }

    /// Sends `mail` to `to`. It is delivered after the turn, in a later step.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: To, mail: &K) -> Result<(), EncodeError> {
        let to = match to {
            To::Sender => To::Held(ErasedActorRef::new(self.source)),
            named => named,
        };

        self.seat.send(&to, mail)
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

    /// Logs `text` at `level`. The line is kept when this turn fails.
    pub fn log(&mut self, level: Level, text: impl Into<String>) {
        self.seat.log(level, text);
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
        Self { seat: Seat { id, path, table, asked, sent: 0 } }
    }

    /// Sends `mail` to `to`. It goes out with the mail of the turn this `begin` runs in.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(&to.into(), mail)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    pub fn spawn<C: Actor + 'static>(&mut self, key: &C::Key, config: C::Config) -> Result<ErasedActorRef, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    pub fn watch(&mut self, of: ErasedActorRef) {
        self.seat.watch(of);
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    pub fn log(&mut self, level: Level, text: impl Into<String>) {
        self.seat.log(level, text);
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
    /// The closing turn of the actor `id` stands for, at `path`, which runs because of `why`, after the actor
    /// has sent `sent` pieces of mail.
    ///
    /// For the engine.
    #[must_use]
    pub const fn new(
        id: ActorId,
        path: ActorPath,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        sent: u64,
        why: Why,
    ) -> Self {
        Self { seat: Seat { id, path, table, asked, sent }, why }
    }

    /// Why this closing turn runs.
    #[must_use]
    pub const fn why(&self) -> Why {
        self.why
    }

    /// Sends `mail` to `to`: the actor's last mail.
    pub fn send<K: Kind + Encode<Actors>>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError> {
        self.seat.send(&to.into(), mail)
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    pub fn log(&mut self, level: Level, text: impl Into<String>) {
        self.seat.log(level, text);
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
