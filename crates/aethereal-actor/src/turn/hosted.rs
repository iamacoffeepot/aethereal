//! A turn a host runs for an actor whose code runs where the engine cannot see it: a guest (ADR-0015).

use super::{Checked, During, Seat, Stood, Turning};
use crate::{
    Above, Asked, Chain, Code, Declared, Effect, ErasedActorRef, Fault, Instances, Level, Line, MailId, Mailed, Role,
    Runs, SeatNumber, Seated, Seen, Sending, Table, Target, What,
};
use aethereal_data::{ActorPath, KeyName, Keyed, KindId, KindName, PathSegment, Plain};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::convert::Infallible;

/// A turn as a host runs it: the guest's sends and lines are handed in after its code ran, each send checked against
/// what the handler that ran declares (ADR-0015).
pub struct Hosted<'a> {
    seat: Seat<'a>,
    /// What the turn that ran may send.
    sends: &'static [Sending],
    /// Who a send to the sender reaches.
    source: Source,
    chain: Chain,
    /// The children the turn spawned, in the order it spawned them, whose `begin`s send and log as themselves.
    spawned: Vec<Spawned>,
}

/// A child a hosted turn spawned: who it is, where it stands, what its type declares, who spawned it, and how many
/// pieces of mail its `begin` has sent.
struct Spawned {
    me: Seated,
    path: ActorPath,
    declared: &'static Declared,
    by: Seated,
    sent: u64,
}

/// Who a hosted turn's sender is.
#[derive(Clone, Copy)]
enum Source {
    /// The actor's `begin`, which handles nothing.
    Begin,
    /// The source of the item handled.
    Item(Seated),
}

/// The reason of a guest's turn that sent what its declaration does not allow (ADR-0013, ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct FalseSend;

impl aethereal_data::Kind for FalseSend {
    const NAME: KindName = KindName::new("aether.actor.false_send");
}

/// The reason of a guest's turn that watched an actor though the watcher's type does not handle endings (ADR-0013,
/// ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct FalseWatch;

impl aethereal_data::Kind for FalseWatch {
    const NAME: KindName = KindName::new("aether.actor.false_watch");
}

/// The reason of a guest's turn that trapped: every actor of its instance ends with it (ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Trapped;

impl aethereal_data::Kind for Trapped {
    const NAME: KindName = KindName::new("aether.actor.trapped");
}

impl<'a> Turning<'a> {
    /// The turn of the handler for `role` of an actor whose type declares `declared`, as a host runs it, and the
    /// instances of its part it was lent, for the host to take the instance its code runs in from.
    #[must_use]
    pub fn hosted(self, declared: &'static Declared, role: Role) -> (Hosted<'a>, &'a mut Instances) {
        let Self { me, path, above, table, made, asked, instances, sent, source, chain } = self;
        let handler = declared.handlers().iter().find(|handler| handler.role() == role);
        let sends = handler.map_or(&[][..], |handler| handler.sends());
        let during = During::Handling(role);
        let seat = Seat { me, path, above, table, made, asked, sent, declared, during, checked: Checked::Nothing };

        (Hosted { seat, sends, source: Source::Item(source), chain, spawned: Vec::new() }, instances)
    }
}

impl<'a> Hosted<'a> {
    /// The `begin` of the root `me` names, at `path`, whose type declares `declared`, reading `table` and adding to
    /// `asked`: for the engine, which makes a root of a guest for the chassis.
    #[must_use]
    pub fn beginning(
        me: Seated,
        path: &'a ActorPath,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        declared: &'static Declared,
    ) -> Self {
        let seat = Seat {
            me,
            path,
            above: Above::Chassis,
            table,
            made: &(),
            asked,
            sent: 0,
            declared,
            during: During::Begin,
            checked: Checked::Nothing,
        };
        let chain = Chain { root: MailId { sender: me.id, count: 0 }, depth: 0 };

        Self { seat, sends: declared.begins(), source: Source::Begin, chain, spawned: Vec::new() }
    }

    /// The actor whose turn this is.
    #[must_use]
    pub const fn me(&self) -> Seated {
        self.seat.me
    }

    /// The source of the item handled: nothing for a `begin`.
    #[must_use]
    pub const fn source(&self) -> Option<Seated> {
        match self.source {
            Source::Begin => None,
            Source::Item(source) => Some(source),
        }
    }

    /// Where the item handled stands in its chain.
    #[must_use]
    pub const fn chain(&self) -> Chain {
        self.chain
    }

    /// How many pieces of mail the actor has sent, this turn's so far included.
    #[must_use]
    pub const fn sent(&self) -> u64 {
        self.seat.sent
    }

    /// Sends `bytes`, mail of `kind`, to the actor `to` names, by `by`: the actor whose turn this is, or a child it
    /// spawned, in its `begin`. Refused when no send the sender declares there reaches it.
    ///
    /// # Errors
    ///
    /// [`FalseSend`] when the sender's declaration does not allow it, or the turn spawned no such child.
    pub fn send(&mut self, by: Seated, to: Seated, kind: KindId, bytes: &[u8]) -> Result<(), FalseSend> {
        let (sends, source) = if by == self.seat.me {
            (self.sends, self.source)
        } else {
            let child = self.spawned.iter().find(|child| child.me == by).ok_or(FalseSend)?;
            (child.declared.begins(), Source::Begin)
        };
        let allowed = sends.iter().any(|sending| self.allows(*sending, source, to, kind));
        if !allowed {
            return Err(FalseSend);
        }
        let written = self.seat.asked.arena.write(|mut out| {
            out.extend_from_slice(bytes);

            (out, Ok::<(), Infallible>(()))
        });
        let Ok(bytes) = written;
        let count = if by == self.seat.me {
            self.seat.sent += 1;
            self.seat.sent
        } else {
            let child = self.spawned.iter_mut().find(|child| child.me == by).ok_or(FalseSend)?;
            child.sent += 1;
            child.sent
        };
        let mailed = Mailed { by, to, count, kind, bytes };
        self.seat.asked.buffers.mailed.push(mailed);

        Ok(())
    }

    /// Spawns, by `by`, the actor whose turn this is or a child it spawned, a child of the type `declared`, under
    /// `key`, in `seat`, as a turn spawns one: the spawn is asked for, and the child is the turn's to stand up with
    /// [`Self::stand`] once its `begin` has run. Answers with the child: nothing when `by` is not who it says, the key
    /// is not of the type's keying, or the key is taken, so the guest's records do not read back.
    pub fn spawn(
        &mut self,
        by: Seated,
        declared: &'static Declared,
        key: Option<KeyName>,
        seat: SeatNumber,
    ) -> Option<Seated> {
        let by_path = if by == self.seat.me {
            self.seat.path.clone()
        } else {
            self.spawned.iter().find(|child| child.me == by)?.path.clone()
        };
        let segment = match (&key, declared.keyed()) {
            (None, Keyed::Singleton) => PathSegment::new(declared.name().clone(), &()),
            (Some(key), Keyed::Instanced) => PathSegment::instanced(declared.name().clone(), key.clone()),
            _ => return None,
        };
        let path = by_path.child(&segment).ok()?;
        let id = self.seat.table.child(by.id, &segment);
        let is_live = self.seat.table.child_seat(id).is_some();
        let is_made = by == self.seat.me && self.seat.made.has_made(id);
        let is_stood = self.seat.asked.stood_seat(id).is_some();
        let is_spawned = self.spawned.iter().any(|child| child.me.id == id);
        let is_taken = is_live || is_made || is_stood || is_spawned;
        if is_taken {
            return None;
        }
        let from = Code::Linked(declared);
        let what = match key {
            None => What::Spawn { from, seat },
            Some(key) => What::SpawnInstanced { from, key, seat },
        };
        self.seat.asked.buffers.effects.push(Effect::new(by, what));
        let me = Seated { id, seat };
        self.spawned.push(Spawned { me, path, declared, by, sent: 0 });

        Some(me)
    }

    /// Stands up `me`, a child [`Self::spawn`] spawned whose `begin` succeeded, run by `runs`: the engine seats it
    /// with the turn's other children. Nothing when the turn spawned no such child.
    pub fn stand(&mut self, me: Seated, runs: Box<dyn Runs>) -> Option<()> {
        let child = self.spawned.iter().find(|child| child.me == me)?;
        let above_declared = if child.by == self.seat.me {
            self.seat.declared
        } else {
            self.spawned.iter().find(|above| above.me == child.by)?.declared
        };
        let above = Above::Actor(child.by, above_declared);
        self.seat.asked.stood_seats.insert(me.id.bits(), (me.seat, child.declared));
        self.seat.asked.stood.push(Stood { me, path: child.path.clone(), above, actor: runs });

        Some(())
    }

    /// Who spawned `me`, a child [`Self::spawn`] spawned in this turn, and what its type declares: nothing when the
    /// turn spawned no such child.
    #[must_use]
    pub fn spawned(&self, me: Seated) -> Option<(Seated, &'static Declared)> {
        let child = self.spawned.iter().find(|child| child.me == me)?;

        Some((child.by, child.declared))
    }

    /// Watches the actor `of` names, by `by`: the actor whose turn this is, or a child it spawned, in its `begin`, as
    /// [`Turn::watch`](crate::Turn::watch) does. Whether `of` was live is read from the table and the children stood
    /// up in the step, as a native watch reads it, not taken from the guest.
    ///
    /// # Errors
    ///
    /// [`FalseWatch`] when the watcher's type does not handle endings, or the turn spawned no such child.
    pub fn watch(&mut self, by: Seated, of: Seated) -> Result<(), FalseWatch> {
        let declared = if by == self.seat.me {
            self.seat.declared
        } else {
            self.spawned.iter().find(|child| child.me == by).ok_or(FalseWatch)?.declared
        };
        if !declared.handles_endings() {
            return Err(FalseWatch);
        }
        let was_held = self.seat.table.held(of.id);
        let is_stood = self.seat.asked.stood_seat(of.id).is_some();
        let seen = if was_held || is_stood {
            Seen::Live
        } else {
            Seen::Ended
        };
        let watch = What::Watch { of: ErasedActorRef::new(of), seen };
        self.seat.asked.buffers.effects.push(Effect::new(by, watch));

        Ok(())
    }

    /// Gives back `seat`, one the turn's host took for a child that was never stood up.
    pub fn give_back(&mut self, seat: SeatNumber) {
        self.seat.asked.seats.give_back(seat);
    }

    /// Whether the turn may close its actor: a turn of an item may, and a `begin` may not.
    #[must_use]
    pub const fn may_close(&self) -> bool {
        matches!(self.source, Source::Item(_))
    }

    /// Closes this actor, as [`Turn::close`](crate::Turn::close) does.
    pub fn close(&mut self) {
        self.seat.ask(What::Close);
    }

    /// Logs `text` at `level`, as `by`: the actor whose turn this is, or a child it spawned, in its `begin`. Nothing
    /// when the turn spawned no such child.
    pub fn log(&mut self, by: Seated, level: Level, text: &str) -> Option<()> {
        let is_child = by != self.seat.me;
        if is_child {
            self.spawned.iter().find(|child| child.me == by)?;
        }
        if let Ok(text) = self.seat.asked.arena.write_text(text) {
            self.seat.asked.buffers.lines.push(Line { by, level, text });
        }

        Some(())
    }

    /// Ends each of `with` but this actor, the actors whose code ran in the instance that trapped in this turn, as a
    /// failed turn of its own would end it: the trap may have left the memory they live in half changed (ADR-0015).
    /// The engine keeps what this asks for when the turn fails, as it keeps the turn's lines.
    pub fn trapped(&mut self, with: impl IntoIterator<Item = Seated>) {
        let Ok(fault) = Fault::new(&Trapped) else {
            return;
        };
        for other in with {
            let is_other = other != self.seat.me;
            if is_other {
                self.seat.asked.buffers.effects.push(Effect::new(other, What::Failed(fault.clone())));
            }
        }
    }

    /// Whether `sending` sends mail of `kind` to the actor `to` names. An actor the table no longer holds has ended,
    /// and its mail is dropped where it would be handed over, so any send of the kind reaches it.
    fn allows(&self, sending: Sending, source: Source, to: Seated, kind: KindId) -> bool {
        let is_kind = sending.kind() == kind;
        if !is_kind {
            return false;
        }
        match sending.to() {
            Target::Sender => matches!(source, Source::Item(source) if source == to),
            Target::Type(_, id) => self.seat.declared_at(to.id).is_none_or(|receiver| receiver.id() == id),
            Target::Handling(kinds) => {
                let receiver = self.seat.declared_at(to.id);
                receiver.is_none_or(|receiver| kinds.iter().all(|kind| receiver.lists(*kind)))
            }
        }
    }
}
