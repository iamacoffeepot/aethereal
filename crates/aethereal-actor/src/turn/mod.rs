//! A turn: what an actor asks of the engine while it handles one item (ADR-0009).

mod refused;
#[cfg(test)]
mod tests;

pub use refused::Refused;

use crate::{
    Actor, ActorRef, Actors, ActorsWriter, Allows, Arena, Children, Code, Declared, Effect, ErasedActorRef, Handled,
    Handles, Kinds, Level, Line, MailId, Mailed, Marker, Role, Runs, SeatNumber, Seated, Seats, Seen, Sends, Table,
    Target, To, ToAny, ToSender, ToType, ToWithoutMail, What,
};
use aethereal_data::{ActorId, ActorPath, Encode, EncodeError, Kind, KindId, PathSegment, PathTooDeep, Writer};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use core::mem;
use core::str;

/// What one turn has asked for, in three lists it fills as it runs: the mail it sent, the lines it logged, and its
/// effects, in the order it asked for each; and the children it stood up (ADR-0009).
///
/// The engine makes one for a turn and takes it back when the turn returns.
/// Nothing in it has happened yet: the engine alone applies what is asked.
pub struct Asked {
    buffers: Buffers,
    stood: Vec<Stood>,
    /// The seats of the children in `stood`, and what their types declare, under the bits of their ids, to find
    /// whether one stands at a key, where it sits, and what it is in a lookup.
    stood_seats: BTreeMap<u64, (SeatNumber, &'static Declared)>,
    /// Where the bytes of the mail the turn sends, and the text of the lines it logs, are written.
    arena: Arena,
    /// The seats the children the turn spawns are given.
    seats: Seats,
}

/// The three buffers of what turns ask for: kept by a worker over its share of a step, and filled by each turn in turn.
#[derive(Debug, Default)]
pub struct Buffers {
    /// The mail sent, in the order sent.
    pub mailed: Vec<Mailed>,
    /// The lines logged, in the order logged.
    pub lines: Vec<Line>,
    /// Everything else asked for: the effects on the lives of actors, in the order asked for.
    pub effects: Vec<Effect>,
}

/// How long each of the buffers was at some moment: where what was asked after it begins.
#[derive(Clone, Copy)]
struct Marks {
    mailed: usize,
    effects: usize,
}

impl Buffers {
    /// How long the buffers a failed `begin` takes back from are.
    fn marks(&self) -> Marks {
        Marks { mailed: self.mailed.len(), effects: self.effects.len() }
    }

    /// Whether every buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let no_mail = self.mailed.is_empty();
        let no_lines = self.lines.is_empty();
        let no_effects = self.effects.is_empty();

        no_mail && no_lines && no_effects
    }
}

impl Asked {
    /// Nothing asked for yet, with mail written to an arena of its own, and children seated as in an engine kept in
    /// one part.
    #[must_use]
    pub fn new() -> Self {
        Self::writing(Arena::default(), Seats::one_part())
    }

    /// Nothing asked for yet, with mail written to `arena`, after what it already holds, and children seated from
    /// `seats`.
    #[must_use]
    pub fn writing(arena: Arena, seats: Seats) -> Self {
        Self::continuing(Buffers::default(), arena, seats)
    }

    /// Nothing asked for yet, in `buffers`, emptied and kept for their room, with mail written to `arena`, after what
    /// it already holds, and children seated from `seats`.
    #[must_use]
    pub fn continuing(buffers: Buffers, arena: Arena, seats: Seats) -> Self {
        debug_assert!(buffers.is_empty(), "a turn begins with its buffers emptied");

        Self { buffers, stood: Vec::new(), stood_seats: BTreeMap::new(), arena, seats }
    }

    /// Begins the next turn, once what the last asked for has been taken out of the buffers. The children the last
    /// turn stood up have been taken with [`Self::take_stood`].
    ///
    /// The engine keeps one of these for a worker's whole share of a step, and not one for each turn, so a turn
    /// moves nothing in or out, and every buffer holds only what the turn running asked for.
    pub fn next_turn(&mut self) {
        debug_assert!(self.buffers.is_empty(), "a turn begins with its buffers emptied");
        // Most turns stand up no child, and clearing even an empty map walks it.
        if !self.stood_seats.is_empty() {
            self.stood_seats.clear();
        }
    }

    /// Whether this turn has asked for nothing in any buffer.
    #[must_use]
    pub fn asked_nothing(&self) -> bool {
        self.buffers.is_empty()
    }

    /// Takes the children this turn stood up, each in the order its `begin` finished.
    pub fn take_stood(&mut self) -> Vec<Stood> {
        mem::take(&mut self.stood)
    }

    /// Every list, earlier turns' entries first, for the engine to add its own to, route, and empty.
    pub fn buffers_mut(&mut self) -> &mut Buffers {
        &mut self.buffers
    }

    /// The seats children are given, for the engine to give back those of children that did not stay.
    pub fn seats_mut(&mut self) -> &mut Seats {
        &mut self.seats
    }

    /// The arena the turn's mail and lines were written to.
    #[must_use]
    pub const fn arena(&self) -> &Arena {
        &self.arena
    }

    /// The mail this turn sent, in the order sent.
    #[must_use]
    pub fn mailed(&self) -> &[Mailed] {
        &self.buffers.mailed
    }

    /// The lines this turn logged, in the order logged.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.buffers.lines
    }

    /// The text of `line`, one this arena holds.
    #[must_use]
    pub fn text(&self, line: &Line) -> &str {
        let bytes = self.arena.read(line.text).unwrap_or_default();

        str::from_utf8(bytes).unwrap_or_default()
    }

    /// This turn's other effects, in the order they were asked for.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.buffers.effects
    }

    /// The lists, earlier turns' entries first when they were continued, the children stood up, each child in the
    /// order its `begin` finished, the arena the mail was written to, and the seats not given out.
    #[must_use]
    pub fn into_parts(self) -> (Buffers, Vec<Stood>, Arena, Seats) {
        (self.buffers, self.stood, self.arena, self.seats)
    }

    /// The seat of the child stood up in this turn under the id `id`, when there is one.
    fn stood_seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.stood_seats.get(&id.bits()).map(|(seat, _)| *seat)
    }

    /// What the type of the child stood up in this turn under the id `id` declares, when there is one.
    fn stood_declared(&self, id: ActorId) -> Option<&'static Declared> {
        self.stood_seats.get(&id.bits()).map(|(_, declared)| *declared)
    }
}

impl Default for Asked {
    /// As [`Asked::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// A child stood up in a turn and held aside until the engine applies its spawn (ADR-0009).
///
/// Until then it exists for no one but its parent.
pub struct Stood {
    me: Seated,
    path: ActorPath,
    above: Above,
    actor: Box<dyn Runs>,
}

/// What stands directly above an actor: the actor [`To::Parent`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Above {
    /// Nothing: the actor is a root, made for the chassis.
    Chassis,
    /// The actor that spawned it, its seat, and what its type declares, which a child's `begin` cannot read from the
    /// table while its parent is still being stood up.
    Actor(Seated, &'static Declared),
}

impl Stood {
    /// The child's id.
    #[must_use]
    pub const fn id(&self) -> ActorId {
        self.me.id
    }

    /// The child, and the seat it was given.
    #[must_use]
    pub const fn seated(&self) -> Seated {
        self.me
    }

    /// Where the child stands.
    #[must_use]
    pub const fn path(&self) -> &ActorPath {
        &self.path
    }

    /// The actor that stood the child up.
    #[must_use]
    pub const fn above(&self) -> Above {
        self.above
    }

    /// What the child's type declares (ADR-0013).
    #[must_use]
    pub fn declared(&self) -> &'static Declared {
        self.actor.declared()
    }

    /// The child itself, for the engine to run.
    #[must_use]
    pub fn into_actor(self) -> Box<dyn Runs> {
        self.actor
    }

    /// The child taken apart, for the engine to seat it: who it is and where it sits, its path, what stands above
    /// it, and the child itself.
    #[must_use]
    pub fn into_parts(self) -> (Seated, ActorPath, Above, Box<dyn Runs>) {
        (self.me, self.path, self.above, self.actor)
    }
}

/// Whose turn it is, and what every kind of turn works with.
struct Seat<'a> {
    /// The actor, and its seat.
    me: Seated,
    path: &'a ActorPath,
    /// What stands above the actor, kept when it was stood up, so a send to its parent reads no path.
    above: Above,
    table: &'a dyn Table,
    /// The children the actor made before this turn's step, whose keys it may not use again.
    made: &'a dyn Children,
    asked: &'a mut Asked,
    /// How many pieces of mail this actor has sent, this turn's included.
    sent: u64,
    /// What the actor's type declares (ADR-0013).
    declared: &'static Declared,
    /// Which of its turns this is, named when a send breaks the declaration.
    during: During,
    /// The last send of the turn that was checked against the declaration, if it is still the turn's last effect.
    checked: Checked,
}

/// Which send of a turn was last checked against the actor's declaration (ADR-0013): a send like it that follows it
/// straight after need not be checked again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Checked {
    /// No send of the turn has been checked.
    Nothing,
    /// The send that was the last in the mail buffer when it held `mailed` pieces, checked against `target`.
    Last { mailed: usize, target: Target },
}

/// Which of an actor's turns runs: named when a send breaks the declaration (ADR-0013).
#[derive(Clone, Copy)]
enum During {
    Begin,
    Handling(Role),
    End,
}

impl fmt::Display for During {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Begin => write!(f, "its begin"),
            Self::Handling(Role::Kind(kind)) => write!(f, "its handler of {kind:?}"),
            Self::Handling(Role::Ended) => write!(f, "its handler of endings"),
            Self::Handling(Role::Others) => write!(f, "its fallback"),
            Self::End => write!(f, "its closing turn"),
        }
    }
}

impl Seat<'_> {
    /// Adds the effect by which this actor asks for `what`.
    fn ask(&mut self, what: What) {
        self.asked.buffers.effects.push(Effect::new(self.me, what));
    }

    /// A reference to this actor.
    const fn me(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.me)
    }

    /// A reference to the live actor `path` names.
    fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.table.resolve(path).map(ErasedActorRef::new)
    }

    /// A handle that logs as this actor and does nothing else.
    const fn logger(&mut self) -> Logger<'_> {
        Logger { me: self.me, asked: self.asked }
    }

    /// Adds the effect that watches the actor `of` refers to, with how this turn sees it: live when the table held
    /// it as the step began or it was stood up since, and ended otherwise, since a reference's actor existed
    /// (ADR-0008).
    ///
    /// # Panics
    ///
    /// Panics when this actor's type does not handle endings: the ending could not be handed to it (ADR-0013).
    fn watch(&mut self, of: ErasedActorRef) {
        let handles_endings = self.declared.handles_endings();
        assert!(
            handles_endings,
            "false declaration: an actor of type {} watched an actor in {}, and its type does not handle endings",
            self.declared.name(),
            self.during,
        );
        let id = of.seated().id;
        let was_held = self.table.held(id);
        let is_stood = self.asked.stood_seat(id).is_some();
        let seen = if was_held || is_stood {
            Seen::Live
        } else {
            Seen::Ended
        };
        self.ask(What::Watch { of, seen });
    }

    /// Encodes `mail` and adds the effect that sends it to the actor `to` names, which `target` says it may reach.
    ///
    /// Nothing is sent when `to` names no actor this turn can see: a parent no longer live, or a root's parent.
    /// Whether the actor it names is live when the mail would be handed to it is the engine's to see, and mail to
    /// one that is not is dropped there.
    fn send<K: Kind + Encode<Actors>>(&mut self, to: &To, target: Target, mail: &K) -> Result<(), EncodeError> {
        let Some(to) = self.receiver(to) else {
            return Ok(());
        };
        // A send to the receiver and of the kind of the turn's last, made by the same declared send, was checked with
        // it. A reply or a send through a reference is never checked, so one that went before proves nothing.
        let is_like_last = self.last_send().is_some_and(|(last, kind)| last == to && kind == KindId::of::<K>());
        let was_checked = self.checked == Checked::Last { mailed: self.asked.buffers.mailed.len(), target };
        let is_checked = is_like_last && was_checked;
        if !is_checked {
            self.check(to, target, KindId::of::<K>());
        }
        self.write_after(to, mail, is_like_last)?;
        self.checked = Checked::Last { mailed: self.asked.buffers.mailed.len(), target };

        Ok(())
    }

    /// The receiver and kind of this turn's last mail, when this actor sent it.
    fn last_send(&self) -> Option<(Seated, KindId)> {
        let last = self.asked.buffers.mailed.last()?;
        let is_mine = last.by.id == self.me.id;

        is_mine.then_some((last.to, last.kind))
    }

    /// Encodes `mail` and adds the effect that sends it to the actor `to`, which this actor reaches as it is: one a
    /// typed reference proves, or the sender.
    fn write<K: Kind + Encode<Actors>>(&mut self, to: Seated, mail: &K) -> Result<(), EncodeError> {
        // A reply goes on its own: a turn rarely answers one sender back to back.
        self.write_after(to, mail, false)
    }

    /// As [`Self::write`], knowing whether `to` and the kind are those of the turn's last send.
    fn write_after<K: Kind + Encode<Actors>>(
        &mut self,
        to: Seated,
        mail: &K,
        is_like_last: bool,
    ) -> Result<(), EncodeError> {
        let bytes = self.asked.arena.write(|bytes| {
            let mut out = Writer::<Actors>::appending(bytes, ActorsWriter::new());
            let written = mail.encode(&mut out);

            (out.into_bytes(), written)
        })?;
        self.sent += 1;
        let kind = KindId::of::<K>();
        // Mail of one kind to one receiver, sent back to back by sends the turn named, is one run: one effect, one item
        // in its stream.
        if is_like_last
            && let Some(last) = self.asked.buffers.mailed.last_mut()
            && last.count + u64::from(last.bytes.pieces()) == self.sent
            && let Some(joined) = last.bytes.joined(bytes)
        {
            last.bytes = joined;
            return Ok(());
        }
        self.asked.buffers.mailed.push(Mailed { by: self.me, to, count: self.sent, kind, bytes });

        Ok(())
    }

    /// Stops the engine when the actor `to` names is not one `target` reaches: the send breaks this actor's
    /// declaration (ADR-0013). A send to the sender reaches it by how it was made.
    ///
    /// # Panics
    ///
    /// Panics when the receiver's type is not the one `target` names, or does not list the kinds it names.
    fn check(&self, to: Seated, target: Target, kind: KindId) {
        // A reply reaches its receiver by how it was made, so nothing is looked up for it.
        if target == Target::Sender {
            return;
        }
        // An actor a reference names that the table no longer holds has ended: the mail is dropped where it would be
        // handed over, and there is no type left to check it against (ADR-0008).
        let Some(receiver) = self.declared_at(to.id) else {
            return;
        };
        let reaches = match target {
            Target::Sender => true,
            Target::Type(_, id) => receiver.id() == id,
            Target::Handling(kinds) => kinds.iter().all(|kind| receiver.lists(*kind)),
        };
        let receiver = receiver.name();
        assert!(
            reaches,
            "false declaration: an actor of type {} sent {kind:?} in {} to {to:?}, of type {receiver:?}, which \
             its declaration of {target:?} does not reach",
            self.declared.name(),
            self.during,
        );
    }

    /// What the type of the actor `id` stands for declares: this actor, its parent, a child this turn stood up, or one
    /// the table holds. An actor's own `begin`, and its parent's, may run before the table holds either.
    fn declared_at(&self, id: ActorId) -> Option<&'static Declared> {
        let is_me = id == self.me.id;
        if is_me {
            return Some(self.declared);
        }
        if let Above::Actor(parent, declared) = self.above {
            let is_parent = id == parent.id;
            if is_parent {
                return Some(declared);
            }
        }

        self.asked.stood_declared(id).or_else(|| self.table.declared(id))
    }

    /// The actor `to` names, beside this one, and its seat: none for a parent this turn cannot see, or a child that
    /// was never made.
    #[inline]
    fn receiver(&self, to: &To) -> Option<Seated> {
        match to {
            To::Parent => self.parent(),
            To::Child(kind) => self.child(&PathSegment::new(kind.clone(), &())),
            To::ChildInstanced(kind, key) => self.child(&PathSegment::instanced(kind.clone(), key.clone())),
            To::Held(reference) => Some(reference.seated()),
            // A turn puts the source of its item in its place before it asks.
            To::Sender => None,
        }
    }

    /// The actor directly above this one, and its seat, when it is live.
    fn parent(&self) -> Option<Seated> {
        let Above::Actor(parent, _) = self.above else {
            return None;
        };
        let is_live = self.table.is_live(parent.id);

        is_live.then_some(parent)
    }

    /// The child `segment` names beneath this actor, and its seat, when it was made: stood up in this turn, or
    /// before.
    fn child(&self, segment: &PathSegment) -> Option<Seated> {
        let id = self.table.child(self.me.id, segment);
        let seat = self.asked.stood_seat(id).or_else(|| self.table.child_seat(id))?;

        Some(Seated { id, seat })
    }

    /// Stands up a child of type `C` under `key`: adds its spawn effect, then runs its `begin` here.
    ///
    /// A key is free when this actor has made no child there: none its seat keeps from earlier steps, none live,
    /// and none stood up earlier in the step or in this turn (ADR-0008).
    fn spawn<C: Actor>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused> {
        let segment = PathSegment::new(C::NAME, key);
        let path = self.path.child(&segment).map_err(|PathTooDeep| Refused::TooDeep)?;
        let id = self.table.child(self.me.id, &segment);
        let is_made = self.made.has_made(id);
        let is_live = self.table.child_seat(id).is_some();
        let is_stood = self.asked.stood_seat(id).is_some();
        let is_taken = is_made || is_live || is_stood;
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        let declared = Declared::of::<C>();
        let from = Code::Linked(declared);
        let seat = self.asked.seats.take();
        let before = self.asked.buffers.marks();
        let spawn = match segment.key() {
            None => What::Spawn { from, seat },
            Some(key) => What::SpawnInstanced { from, key: key.clone(), seat },
        };
        self.ask(spawn);

        let me = Seated { id, seat };
        let stood_before = self.asked.stood.len();
        let above = Above::Actor(self.me, self.declared);
        let table = self.table;
        let asked = &mut *self.asked;
        let child = Seat {
            me,
            path: &path,
            above,
            table,
            made: &(),
            asked,
            sent: 0,
            declared,
            during: During::Begin,
            checked: Checked::Nothing,
        };
        match C::begin(config, &mut Beginning { seat: child, sends: PhantomData }) {
            Ok(actor) => {
                self.asked.stood_seats.insert(id.bits(), (seat, declared));
                self.asked.stood.push(Stood { me, path, above, actor: Box::new(actor) });

                Ok(ActorRef::new(me))
            }
            Err(fault) => {
                // The child and every child it stood up never began, so their seats are given back, and what they
                // asked for, the spawn of the child among it, is taken back but for the lines they logged.
                self.asked.buffers.mailed.truncate(before.mailed);
                self.asked.buffers.effects.truncate(before.effects);
                for below in self.asked.stood.drain(stood_before..) {
                    self.asked.stood_seats.remove(&below.me.id.bits());
                    self.asked.seats.give_back(below.me.seat);
                }
                self.asked.seats.give_back(seat);

                Err(Refused::Began(fault))
            }
        }
    }
}

/// What logs as one actor in its turn, and does nothing else (ADR-0009): lent by a turn to a helper that logs.
pub struct Logger<'t> {
    me: Seated,
    asked: &'t mut Asked,
}

impl Logger<'_> {
    /// Logs `text` at `level`, as the turn that lent this does. The line is kept when the turn fails. A text whose
    /// `Display` fails logs nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        if let Ok(text) = self.asked.arena.write_text(text) {
            self.asked.buffers.lines.push(Line { by: self.me, level, text });
        }
    }
}

/// Where the mail a turn sends to an actor of type `T` goes: an actor its turn may send to, as [`ToType`] declares
/// (ADR-0013).
pub struct Address<'t, 'a, T, S> {
    seat: &'t mut Seat<'a>,
    to: Receiver,
    declared: PhantomData<fn() -> (T, S)>,
}

/// Who an [`Address`] sends to.
enum Receiver {
    /// The actor a [`To`] names, whose type is checked when it is sent to.
    Named(To),
    /// The actor a typed reference refers to, whose type the reference proves.
    Proven(Seated),
}

impl<T: Actor, S> Address<'_, '_, T, S> {
    /// Sends `mail` to the actor. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list [`ToType<T, K>`].
    ///
    /// # Panics
    ///
    /// Panics when the actor a [`To`] named is not of type `T` (ADR-0013). An actor a typed reference refers to is.
    pub fn send<K, I>(self, mail: &K) -> Result<(), EncodeError>
    where
        K: Kind + Encode<Actors> + Handled,
        T: Handles<K>,
        S: Allows<ToType<T, K>, I>,
    {
        match &self.to {
            Receiver::Named(to) => self.seat.send(to, <ToType<T, K> as Marker>::SENDING.to(), mail),
            Receiver::Proven(to) => self.seat.write(*to, mail),
        }
    }
}

/// Where the mail a turn sends to any actor that handles the kinds of `P` goes, as [`ToAny`] declares (ADR-0013).
pub struct AnyAddress<'t, 'a, P, S> {
    seat: &'t mut Seat<'a>,
    to: To,
    declared: PhantomData<fn() -> (P, S)>,
}

impl<P: Kinds, S> AnyAddress<'_, '_, P, S> {
    /// Sends `mail` to the actor. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list [`ToAny<P, K>`].
    ///
    /// # Panics
    ///
    /// Panics when the actor named does not list every kind of `P` (ADR-0013).
    pub fn send<K, I>(self, mail: &K) -> Result<(), EncodeError>
    where
        K: Kind + Encode<Actors>,
        S: Allows<ToAny<P, K>, I>,
    {
        self.seat.send(&self.to, <ToAny<P, K> as Marker>::SENDING.to(), mail)
    }
}

/// Where in its chain the item a turn handles came: the chain's first mail, and how far down it the item is
/// (ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chain {
    /// The first mail of the chain.
    pub root: MailId,
    /// How far down the chain the item is. Mail the turn sends is one deeper.
    pub depth: u32,
}

/// A turn as the engine opens it, before the handler it runs is known: what [`Runs::handle`] is given.
pub struct Turning<'a> {
    me: Seated,
    path: &'a ActorPath,
    above: Above,
    table: &'a dyn Table,
    made: &'a dyn Children,
    asked: &'a mut Asked,
    sent: u64,
    source: Seated,
    chain: Chain,
}

impl<'a> Turning<'a> {
    /// The turn of the actor `me` names, at `path`, reading `table` and adding to `asked`, which handles an item
    /// from the actor `source` names, at `chain`, and has sent `sent` pieces of mail before it.
    ///
    /// For the engine. `table` answers for the step's start and for what this actor has spawned since.
    #[allow(clippy::too_many_arguments, reason = "what a turn is opened with, each fixed by the engine")]
    #[must_use]
    pub const fn new(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        made: &'a dyn Children,
        asked: &'a mut Asked,
        sent: u64,
        source: Seated,
        chain: Chain,
    ) -> Self {
        Self { me, path, above, table, made, asked, sent, source, chain }
    }

    /// The turn of the handler of `A` for `role`, which may send `S`.
    pub(crate) fn declaring<A: Actor, S: Sends>(self, role: Role) -> Turn<'a, S> {
        let Self { me, path, above, table, made, asked, sent, source, chain } = self;
        let declared = Declared::of::<A>();
        let seat = Seat {
            me,
            path,
            above,
            table,
            made,
            asked,
            sent,
            declared,
            during: During::Handling(role),
            checked: Checked::Nothing,
        };

        Turn { seat, source, chain, sends: PhantomData }
    }
}

/// What an actor asks of the engine while it handles one item, through a handler that may send `S` (ADR-0009,
/// ADR-0013).
///
/// Each ask adds an effect to the turn's buffer of its class, and nothing asked for happens
/// until the turn has returned `Ok`. Each read answers from the table of
/// actors as it stood when the turn's step began.
pub struct Turn<'a, S> {
    seat: Seat<'a>,
    /// The actor the item being handled comes from: who [`To::Sender`] names.
    source: Seated,
    chain: Chain,
    sends: PhantomData<fn() -> S>,
}

impl<'a, S> Turn<'a, S> {
    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    ///
    /// Watching an actor already watched changes nothing.
    ///
    /// # Panics
    ///
    /// Panics when this actor's type does not handle [`Ended`](crate::Ended) (ADR-0013).
    pub fn watch(&mut self, of: impl Into<ErasedActorRef>) {
        self.seat.watch(of.into());
    }

    /// Where mail to the actor `to` names goes, when it is of type `T`.
    pub fn to<T: Actor>(&mut self, to: To) -> Address<'_, 'a, T, S> {
        let to = match to {
            To::Sender => To::Held(ErasedActorRef::new(self.source)),
            named => named,
        };

        Address { seat: &mut self.seat, to: Receiver::Named(to), declared: PhantomData }
    }

    /// Where mail to the actor `reference` refers to goes. Its type is proven, so nothing is checked when it is sent.
    pub fn to_actor<T: Actor>(&mut self, reference: ActorRef<T>) -> Address<'_, 'a, T, S> {
        Address { seat: &mut self.seat, to: Receiver::Proven(reference.seated()), declared: PhantomData }
    }

    /// Where mail to the actor `to` names goes, when its type lists every kind of `P`.
    pub fn to_any<P: Kinds>(&mut self, to: To) -> AnyAddress<'_, 'a, P, S> {
        let to = match to {
            To::Sender => To::Held(ErasedActorRef::new(self.source)),
            named => named,
        };

        AnyAddress { seat: &mut self.seat, to, declared: PhantomData }
    }

    /// Sends `mail` to the source of the item being handled. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list [`ToSender<K>`].
    pub fn reply<K: Kind + Encode<Actors>, I>(&mut self, mail: &K) -> Result<(), EncodeError>
    where
        S: Allows<ToSender<K>, I>,
    {
        self.seat.write(self.source, mail)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    ///
    /// The child takes its own mail. It is live once the engine applies this turn's effects.
    pub fn spawn<C: Actor>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Closes this actor. Its closing turn runs in the next step.
    pub fn close(&mut self) {
        self.seat.ask(What::Close);
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// The source of the item being handled: the sender of mail, or the actor that ended.
    #[must_use]
    pub const fn sender(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.source)
    }

    /// Where in its chain the item being handled came (ADR-0012).
    #[must_use]
    pub const fn chain(&self) -> Chain {
        self.chain
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }
}

/// What an actor asks of the engine in its `begin`, which may send `S`: a first turn, which offers less than a turn
/// (ADR-0009, ADR-0013).
///
/// It offers no close: an actor does not end in the turn that makes it. It is handling no mail, so it has no
/// sender to send to.
pub struct Beginning<'a, S> {
    seat: Seat<'a>,
    sends: PhantomData<fn() -> S>,
}

impl<'a, S: Sends> Beginning<'a, S> {
    /// The `begin` of the root of type `A` that `me` names, at `path`, reading `table` and adding to `asked`.
    ///
    /// For the engine, which makes a root for the chassis. A child's is made by the turn that spawns it.
    #[must_use]
    pub fn new<A: Actor<BeginSends = S>>(
        me: Seated,
        path: &'a ActorPath,
        table: &'a dyn Table,
        asked: &'a mut Asked,
    ) -> Self {
        let declared = Declared::of::<A>();
        let during = During::Begin;
        let seat = Seat {
            me,
            path,
            above: Above::Chassis,
            table,
            made: &(),
            asked,
            sent: 0,
            declared,
            during,
            checked: Checked::Nothing,
        };

        Self { seat, sends: PhantomData }
    }
}

impl<'a, S> Beginning<'a, S> {
    /// Where mail to the actor `to` names goes, when it is of type `T`. It goes out with the mail of the turn this
    /// `begin` runs in.
    pub fn to<T: Actor>(&mut self, to: ToWithoutMail) -> Address<'_, 'a, T, S> {
        Address { seat: &mut self.seat, to: Receiver::Named(to.into()), declared: PhantomData }
    }

    /// Where mail to the actor `reference` refers to goes. Its type is proven, so nothing is checked when it is sent.
    pub fn to_actor<T: Actor>(&mut self, reference: ActorRef<T>) -> Address<'_, 'a, T, S> {
        Address { seat: &mut self.seat, to: Receiver::Proven(reference.seated()), declared: PhantomData }
    }

    /// Where mail to the actor `to` names goes, when its type lists every kind of `P`.
    pub fn to_any<P: Kinds>(&mut self, to: ToWithoutMail) -> AnyAddress<'_, 'a, P, S> {
        AnyAddress { seat: &mut self.seat, to: to.into(), declared: PhantomData }
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    pub fn spawn<C: Actor>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn::<C>(key, config)
    }

    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    ///
    /// # Panics
    ///
    /// Panics when this actor's type does not handle [`Ended`](crate::Ended) (ADR-0013).
    pub fn watch(&mut self, of: impl Into<ErasedActorRef>) {
        self.seat.watch(of.into());
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
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

/// A closing turn as the engine opens it, before the actor's type is known: what [`Runs::end`] is given.
pub struct Closing<'a> {
    me: Seated,
    path: &'a ActorPath,
    above: Above,
    table: &'a dyn Table,
    asked: &'a mut Asked,
    sent: u64,
    why: Why,
}

impl<'a> Closing<'a> {
    /// The closing turn of the actor `me` names, at `path`, which runs because of `why`, after the actor has sent
    /// `sent` pieces of mail.
    ///
    /// For the engine.
    #[must_use]
    pub const fn new(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        sent: u64,
        why: Why,
    ) -> Self {
        Self { me, path, above, table, asked, sent, why }
    }

    /// The closing turn of an actor of type `A`.
    pub(crate) fn declaring<A: Actor>(self) -> Ending<'a, A::EndSends> {
        let Self { me, path, above, table, asked, sent, why } = self;
        let declared = Declared::of::<A>();
        let seat = Seat {
            me,
            path,
            above,
            table,
            made: &(),
            asked,
            sent,
            declared,
            during: During::End,
            checked: Checked::Nothing,
        };

        Ending { seat, why, sends: PhantomData }
    }
}

/// What an actor asks of the engine in its closing turn, which may send `S` (ADR-0009, ADR-0013).
///
/// It offers no spawn and no close. It is handling no mail, so it has no sender to send to.
pub struct Ending<'a, S> {
    seat: Seat<'a>,
    why: Why,
    sends: PhantomData<fn() -> S>,
}

impl<'a, S> Ending<'a, S> {
    /// Why this closing turn runs.
    #[must_use]
    pub const fn why(&self) -> Why {
        self.why
    }

    /// Where mail to the actor `to` names goes, when it is of type `T`: the actor's last mail.
    pub fn to<T: Actor>(&mut self, to: ToWithoutMail) -> Address<'_, 'a, T, S> {
        Address { seat: &mut self.seat, to: Receiver::Named(to.into()), declared: PhantomData }
    }

    /// Where mail to the actor `reference` refers to goes: the actor's last mail. Its type is proven, so nothing is
    /// checked when it is sent.
    pub fn to_actor<T: Actor>(&mut self, reference: ActorRef<T>) -> Address<'_, 'a, T, S> {
        Address { seat: &mut self.seat, to: Receiver::Proven(reference.seated()), declared: PhantomData }
    }

    /// Where mail to the actor `to` names goes, when its type lists every kind of `P`.
    pub fn to_any<P: Kinds>(&mut self, to: ToWithoutMail) -> AnyAddress<'_, 'a, P, S> {
        AnyAddress { seat: &mut self.seat, to: to.into(), declared: PhantomData }
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
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
