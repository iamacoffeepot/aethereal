//! A turn a host runs for an actor whose code runs where the engine cannot see it: a guest (ADR-0015).

use super::{Checked, During, Seat, Turning};
use crate::{Above, Asked, Chain, Declared, Level, MailId, Mailed, Role, Seated, Sending, Table, Target};
use aethereal_data::{ActorPath, KindId, KindName, Plain};
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

/// The reason of a guest's turn that trapped: every actor of its instance ends with it (ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Trapped;

impl aethereal_data::Kind for Trapped {
    const NAME: KindName = KindName::new("aether.actor.trapped");
}

impl<'a> Turning<'a> {
    /// The turn of the handler for `role` of an actor whose type declares `declared`, as a host runs it.
    #[must_use]
    pub fn hosted(self, declared: &'static Declared, role: Role) -> Hosted<'a> {
        let Self { me, path, above, table, made, asked, sent, source, chain } = self;
        let handler = declared.handlers().iter().find(|handler| handler.role() == role);
        let sends = handler.map_or(&[][..], |handler| handler.sends());
        let during = During::Handling(role);
        let seat = Seat { me, path, above, table, made, asked, sent, declared, during, checked: Checked::Nothing };

        Hosted { seat, sends, source: Source::Item(source), chain }
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

        Self { seat, sends: declared.begins(), source: Source::Begin, chain }
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

    /// Sends `bytes`, mail of `kind`, to the actor `to` names: refused when no send the turn declares reaches it.
    ///
    /// # Errors
    ///
    /// [`FalseSend`] when the turn's declaration does not allow it.
    pub fn send(&mut self, to: Seated, kind: KindId, bytes: &[u8]) -> Result<(), FalseSend> {
        let allowed = self.sends.iter().any(|sending| self.allows(*sending, to, kind));
        if !allowed {
            return Err(FalseSend);
        }
        let written = self.seat.asked.arena.write(|mut out| {
            out.extend_from_slice(bytes);

            (out, Ok::<(), Infallible>(()))
        });
        let Ok(bytes) = written;
        self.seat.sent += 1;
        self.seat.asked.turn_sent = self.seat.sent;
        let mailed = Mailed { by: self.seat.me, to, count: self.seat.sent, kind, bytes };
        self.seat.asked.buffers.mailed.push(mailed);

        Ok(())
    }

    /// Logs `text` at `level`, as this actor.
    pub fn log(&mut self, level: Level, text: &str) {
        self.seat.logger().log(level, text);
    }

    /// Whether `sending` sends mail of `kind` to the actor `to` names. An actor the table no longer holds has ended,
    /// and its mail is dropped where it would be handed over, so any send of the kind reaches it.
    fn allows(&self, sending: Sending, to: Seated, kind: KindId) -> bool {
        let is_kind = sending.kind() == kind;
        if !is_kind {
            return false;
        }
        match sending.to() {
            Target::Sender => matches!(self.source, Source::Item(source) if source == to),
            Target::Type(_, id) => self.seat.declared_at(to.id).is_none_or(|receiver| receiver.id() == id),
            Target::Handling(kinds) => {
                let receiver = self.seat.declared_at(to.id);
                receiver.is_none_or(|receiver| kinds.iter().all(|kind| receiver.lists(*kind)))
            }
        }
    }
}
