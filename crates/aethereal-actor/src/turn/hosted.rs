//! A turn a host runs for an actor whose code runs where the engine cannot see it: a guest (ADR-0015).

use super::{Checked, Closing, During, Owing, Seat, Stood, Turning, Why};
use crate::{
    Above, Asked, Asking, Chain, Claim, Claimed, Code, Declared, Effect, ErasedActorRef, Fault, Gift, Giver, Instances,
    Keeping, Keeps, Level, Line, MailId, Mailed, NotHeld, Payload, Puts, Role, Runs, SeatNumber, Seated, Seats, Seen,
    Sending, Table, Target, Tie, What, carry, take_claimed,
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
    /// Where the pieces of mail handed in lie among what the turn asked for, for what the guest claims goes with one
    /// (ADR-0010).
    sent: Sent,
}

/// Where a hosted turn's pieces of mail lie in its mail buffer: one after another, from the first handed in, as
/// nothing else is added between them.
#[derive(Clone, Copy)]
enum Sent {
    /// None has been handed in.
    Nothing,
    /// The first lies at `from`, and `pieces` have been.
    From { from: usize, pieces: usize },
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
    /// The actor's closing turn, which handles nothing.
    End,
}

/// What a piece of mail a guest's code sent is to a request, as its code says: its host checks it (ADR-0017).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    /// Nothing the code says: whether it replies to the item handled is told from what its handler declares.
    Not,
    /// It asks a request, to be answered with a kind `answer` names, keeping `context`, its context's bytes.
    Asks { answer: KindId, context: &'a [u8] },
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

/// The reason of a guest's call that trapped: every actor of its instance ends with it (ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Trapped;

impl aethereal_data::Kind for Trapped {
    const NAME: KindName = KindName::new("aether.actor.trapped");
}

/// The reason of a guest's call that wrote what does not read back: every actor of its instance ends with it
/// (ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Garbled;

impl aethereal_data::Kind for Garbled {
    const NAME: KindName = KindName::new("aether.actor.garbled");
}

/// The reason of a guest's call that changed its references to a blob it was not given, or dropped more of them than
/// it held: every actor of its instance ends with it (ADR-0014, ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Forged;

impl aethereal_data::Kind for Forged {
    const NAME: KindName = KindName::new("aether.actor.forged");
}

/// Why a call into a guest broke, so that nothing runs in its instance again: its host drops the instance, and every
/// actor of it ends with this reason (ADR-0015).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Broke {
    /// The code trapped.
    Trapped,
    /// What the call wrote does not read back, or names what the call did not make.
    Garbled,
    /// The call changed its references to a blob it was not given, or dropped more of them than it held.
    Forged,
}

impl Broke {
    /// The fault every actor of the instance ends with.
    ///
    /// # Panics
    ///
    /// Never: each reason holds nothing, so it is always written.
    #[must_use]
    pub fn fault(self) -> Fault {
        let fault = match self {
            Self::Trapped => Fault::new(&Trapped),
            Self::Garbled => Fault::new(&Garbled),
            Self::Forged => Fault::new(&Forged),
        };
        let Ok(fault) = fault else {
            panic!("a reason that holds nothing is written");
        };

        fault
    }
}

impl<'a> Turning<'a> {
    /// The turn of the handler for `role` of an actor whose type declares `declared`, as a host runs it, and the
    /// instances of its part it was lent, for the host to take the instance its code runs in from.
    #[must_use]
    pub fn hosted(self, declared: &'static Declared, role: Role) -> (Hosted<'a>, &'a mut Instances) {
        let Self { me, path, above, table, made, asked, instances, keeps, sent, source, chain, tie } = self;
        let handler = declared.handlers().iter().find(|handler| handler.role() == role);
        let sends = handler.map_or(&[][..], |handler| handler.sends());
        let during = During::Handling(role);
        let checked = Checked::Nothing;
        let owing = Owing::Item(tie);
        let seat = Seat { me, path, above, table, made, asked, keeps, sent, declared, during, checked, owing };

        (
            Hosted { seat, sends, source: Source::Item(source), chain, spawned: Vec::new(), sent: Sent::Nothing },
            instances,
        )
    }
}

impl<'a> Closing<'a> {
    /// The closing turn of an actor whose type declares `declared`, as a host runs it: the turn, which may send what
    /// the type's `end` declares, why it runs, and the instances of its part, for the host to take the instance its
    /// code runs in from (ADR-0009, ADR-0015).
    #[must_use]
    pub fn hosted(self, declared: &'static Declared) -> (Hosted<'a>, Why, &'a mut Instances) {
        let Self { me, path, above, table, asked, instances, keeps, sent, why } = self;
        let during = During::End;
        let checked = Checked::Nothing;
        let owing = Owing::NOTHING;
        let seat = Seat { me, path, above, table, made: &(), asked, keeps, sent, declared, during, checked, owing };
        let chain = Chain { root: MailId { sender: me.id, count: sent }, depth: 0 };
        let hosted = Hosted {
            seat,
            sends: declared.ends(),
            source: Source::End,
            chain,
            spawned: Vec::new(),
            sent: Sent::Nothing,
        };

        (hosted, why, instances)
    }
}

impl<'a> Hosted<'a> {
    /// The `begin` of the actor `me` names, at `path`, beneath `above`, whose type declares `declared`, reading
    /// `table`, adding to `asked`, and keeping what it takes in `keeping`: for a host, which begins a root or a child of
    /// its guest's code.
    #[must_use]
    pub fn beginning(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        declared: &'static Declared,
        keeping: &'a mut Keeping,
    ) -> Self {
        let seat = Seat {
            me,
            path,
            above,
            table,
            made: &(),
            asked,
            keeps: Keeps::Here(keeping),
            sent: 0,
            declared,
            during: During::Begin,
            checked: Checked::Nothing,
            owing: Owing::NOTHING,
        };
        let chain = Chain { root: MailId { sender: me.id, count: 0 }, depth: 0 };

        Self { seat, sends: declared.begins(), source: Source::Begin, chain, spawned: Vec::new(), sent: Sent::Nothing }
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
            Source::Begin | Source::End => None,
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
    /// spawned, in its `begin`. The mail holds `held`, a pin on each blob its bytes refer to, each at where its
    /// reference begins in them, as a native send's does (ADR-0014). It is to a request what `request` says: one it
    /// asks is filed for the engine as a native turn's is (ADR-0017). Refused when no send the sender declares there
    /// reaches it.
    ///
    /// # Errors
    ///
    /// [`FalseSend`] when the sender's declaration does not allow it, the turn spawned no such child, or it asks a
    /// request where the actor's turn does not, or whose answer its type does not handle.
    #[inline]
    pub fn send(
        &mut self,
        by: Seated,
        to: Seated,
        kind: KindId,
        bytes: &[u8],
        request: Request<'_>,
        #[cfg(feature = "std")] held: &[crate::Pinning],
    ) -> Result<(), FalseSend> {
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
        // Most mail refers to no blob, and is written with nothing kept beside it.
        #[cfg(feature = "std")]
        let bytes = if held.is_empty() {
            self.write(bytes)
        } else {
            self.write_holding(bytes, held)
        };
        #[cfg(not(feature = "std"))]
        let bytes = self.write(bytes);
        let count = if by == self.seat.me {
            self.seat.sent += 1;
            self.seat.sent
        } else {
            let child = self.spawned.iter_mut().find(|child| child.me == by).ok_or(FalseSend)?;
            child.sent += 1;
            child.sent
        };
        // A request is the mail that asks it, so it is counted as that mail is.
        let tie = match request {
            Request::Not => self.tie(by, to, kind)?,
            Request::Asks { answer, context } => self.ask(by, count, answer, context)?,
        };
        let mailed = Mailed { by, to, count, kind, bytes, tie };
        let pieces = self.seat.asked.buffers.mailed.len();
        self.seat.asked.buffers.mailed.push(mailed);
        self.sent = match self.sent {
            Sent::Nothing => Sent::From { from: pieces, pieces: 1 },
            Sent::From { from, pieces } => Sent::From { from, pieces: pieces + 1 },
        };

        Ok(())
    }

    /// Holds the guest to `claim`, what its code claims it did with a resource in this turn, or in the `begin` of a
    /// child it spawned in it: moves the resource as a native turn's call would have, the claims in the order made,
    /// after every send of the turn has been handed in, so a claim that a resource goes with a piece of mail names
    /// the `send`th of them (ADR-0010, ADR-0015).
    ///
    /// # Errors
    ///
    /// [`NotHeld`] when the claim is false: the claimant holds no such resource, none came to it so, or the mail it
    /// names was not sent by it.
    pub fn claim(&mut self, claim: Claim) -> Result<(), NotHeld> {
        let Claim { by, ticket, what } = claim;
        match what {
            Claimed::Took => {
                let Source::Item(source) = self.source else {
                    return Err(NotHeld);
                };
                let is_mine = by == self.seat.me;
                let took = is_mine && self.keeping_of(by).ok_or(NotHeld)?.take_claimed(ticket, source);
                if !took {
                    return Err(NotHeld);
                }
            }
            Claimed::TookConfig => {
                let giver = Giver::above(self.above_of(by).ok_or(NotHeld)?);
                let kept = take_claimed(&mut self.seat.asked.buffers.gifts, giver, ticket).ok_or(NotHeld)?;
                self.keeping_of(by).ok_or(NotHeld)?.adopt(kept);
            }
            Claimed::Gave => {
                let kept = self.keeping_of(by).ok_or(NotHeld)?.give_up_claimed(ticket).ok_or(NotHeld)?;
                self.seat.asked.buffers.gifts.push(Gift::loose(by, kept));
            }
            Claimed::Carried { send } => {
                let mailed = self.sent_piece(send).ok_or(NotHeld)?;
                let (to, mail) = (mailed.to, MailId { sender: mailed.by.id, count: mailed.count });
                let is_theirs = mail.sender == by.id;
                if !is_theirs {
                    return Err(NotHeld);
                }
                carry(&mut self.seat.asked.buffers.gifts, &[ticket], by, to, mail).map_err(|_| NotHeld)?;
            }
            // Its host handed the resource to the child as it began it, before any claim was held to.
            Claimed::Configured => return Err(NotHeld),
        }

        Ok(())
    }

    /// What a send by `by` to `to` of mail of `kind` is to a request: the answer to the item handled, when that is a
    /// request this turn has not answered and the send is a reply its handler declares (ADR-0017). A second answer is
    /// a false send.
    fn tie(&mut self, by: Seated, to: Seated, kind: KindId) -> Result<Tie, FalseSend> {
        let is_mine = by == self.seat.me;
        let is_to_source = matches!(self.source, Source::Item(source) if source == to);
        let replies = self.sends.iter().any(|sending| sending.to() == Target::Sender && sending.kind() == kind);
        let is_reply = is_mine && is_to_source && replies;
        if !is_reply {
            return Ok(Tie::TELL);
        }
        let (tie, owing) = self.seat.owing.reply().map_err(|_| FalseSend)?;
        self.seat.owing = owing;

        Ok(tie)
    }

    /// What the `count`th piece of mail `by` sends carries as the request it asks, to be answered with a kind `answer`
    /// names, filed with `context`: refused unless it is the actor's own turn that handles an item, as only such a turn
    /// asks, and its type handles the answer (ADR-0017). Kept out of line: most mail asks nothing, and inlined it
    /// keeps the send from being inlined where each is read.
    #[cold]
    #[inline(never)]
    fn ask(&mut self, by: Seated, count: u64, answer: KindId, context: &[u8]) -> Result<Tie, FalseSend> {
        let is_mine = by == self.seat.me;
        let is_handling = matches!(self.source, Source::Item(_));
        let handles = self.seat.declared.handles_answer(answer);
        let asks = is_mine && is_handling && handles;
        if !asks {
            return Err(FalseSend);
        }
        self.seat.asked.asks.push(Asking { count, answer, context: context.to_vec() });

        Ok(Tie::asks(count))
    }

    /// The `send`th piece of mail handed in, counting from nought.
    fn sent_piece(&self, send: u32) -> Option<&Mailed> {
        let Sent::From { from, pieces } = self.sent else {
            return None;
        };
        let send = usize::try_from(send).ok()?;
        let is_sent = send < pieces;

        is_sent.then(|| &self.seat.asked.buffers.mailed[from + send])
    }

    /// What `by` keeps: the actor whose turn this is, or a child its guest's code stood up in it.
    fn keeping_of(&mut self, by: Seated) -> Option<&mut Keeping> {
        if by == self.seat.me {
            return match &mut self.seat.keeps {
                Keeps::Here(keeping) => Some(&mut **keeping),
                Keeps::Claimed => None,
            };
        }

        self.seat.asked.stood.iter_mut().find(|child| child.me == by).map(|child| &mut child.keeping)
    }

    /// What stands above `by`: the actor whose turn this is, or a child its guest's code stood up in it.
    fn above_of(&self, by: Seated) -> Option<Above> {
        if by == self.seat.me {
            return Some(self.seat.above);
        }

        self.seat.asked.stood.iter().find(|child| child.me == by).map(|child| child.above)
    }

    /// Writes `bytes`, a piece of mail its guest wrote, into the turn's arena.
    fn write(&mut self, bytes: &[u8]) -> Payload {
        let written = self.seat.asked.arena.write(|mut out| {
            out.extend_from_slice(bytes);

            (out, Ok::<(), Infallible>(()))
        });
        let Ok(payload) = written;

        payload
    }

    /// Writes `bytes`, a piece of mail its guest wrote, into the turn's arena, kept with `held`, a pin on each blob
    /// they refer to, each at where its reference begins in them.
    #[cfg(feature = "std")]
    fn write_holding(&mut self, bytes: &[u8], held: &[crate::Pinning]) -> Payload {
        let written = self.seat.asked.arena.write_holding(|mut out, pins| {
            let base = u32::try_from(out.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(bytes);
            pins.extend(held.iter().map(|(at, pin)| (base + at, pin.clone())));

            (out, Ok::<(), Infallible>(()))
        });
        let Ok(payload) = written;

        payload
    }

    /// Spawns, by `by`, the actor whose turn this is or a child it spawned, a child of the type `from` makes, under
    /// `key`, in `seat`, as a turn spawns one: the spawn is asked for, and the child is the turn's to stand up with
    /// [`Self::stand`] once its `begin` has run, or [`Self::take_in`] when its host ran that apart. Answers with the
    /// child: nothing when `by` is not who it says, the key is not of the type's keying, or the key is taken, so the
    /// guest's records do not read back.
    pub fn spawn(&mut self, by: Seated, from: Code, key: Option<KeyName>, seat: SeatNumber) -> Option<Seated> {
        let declared = from.declared();
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
        let what = key.map_or(What::Spawn { from, seat }, |key| What::SpawnInstanced { from, key, seat });
        self.seat.asked.buffers.effects.push(Effect::new(by, what));
        let me = Seated { id, seat };
        self.spawned.push(Spawned { me, path, declared, by, sent: 0 });

        Some(me)
    }

    /// Stands up `me`, a child [`Self::spawn`] spawned whose `begin` succeeded, run by `runs`: the engine seats it
    /// with the turn's other children. Nothing when the turn spawned no such child.
    pub fn stand(&mut self, me: Seated, runs: Box<dyn Runs>) -> Option<()> {
        self.stand_keeping(me, runs, Keeping::default())
    }

    /// Stands up `me` as [`Self::stand`] does, keeping `keeping`, what its `begin` kept and took.
    fn stand_keeping(&mut self, me: Seated, runs: Box<dyn Runs>, keeping: Keeping) -> Option<()> {
        let child = self.spawned.iter().find(|child| child.me == me)?;
        let above_declared = if child.by == self.seat.me {
            self.seat.declared
        } else {
            self.spawned.iter().find(|above| above.me == child.by)?.declared
        };
        let above = Above::Actor(child.by, above_declared);
        self.seat.asked.stood_seats.insert(me.id.bits(), (me.seat, child.declared));
        self.seat.asked.stood.push(Stood { me, path: child.path.clone(), above, actor: runs, keeping });

        Some(())
    }

    /// Stands up `me`, a child [`Self::spawn`] spawned that is of other code than the turn's, run by `runs` and keeping
    /// `keeping`: its host began it apart while the turn's code waited on a call out, into `asked`, and what that
    /// `begin` asked for is taken into this turn's, ahead of the child (ADR-0015). Nothing when the turn spawned no such
    /// child.
    #[cfg(feature = "std")]
    pub fn take_in(&mut self, me: Seated, asked: Asked, runs: Box<dyn Runs>, keeping: Keeping) -> Option<()> {
        self.spawned.iter().find(|child| child.me == me)?;
        self.seat.asked.take_in(asked);

        self.stand_keeping(me, runs, keeping)
    }

    /// Keeps the lines a child's `begin` logged into `asked`, one its host ran apart that was refused or taken back:
    /// a refused child's lines are kept, as natively.
    pub fn take_lines(&mut self, asked: &Asked) {
        self.seat.asked.take_lines(asked);
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
    /// [`FalseWatch`] when what the watcher's turn may do does not list a watch, its type does not handle endings, or
    /// the turn spawned no such child: what a guest's library would not have built (ADR-0013).
    pub fn watch(&mut self, by: Seated, of: Seated) -> Result<(), FalseWatch> {
        let (declared, sends) = if by == self.seat.me {
            (self.seat.declared, self.sends)
        } else {
            let child = self.spawned.iter().find(|child| child.me == by).ok_or(FalseWatch)?;
            (child.declared, child.declared.begins())
        };
        let declares_watch = sends.iter().any(|sending| sending.to() == Target::Watched);
        let handles_endings = declared.handles_endings();
        let holds = declares_watch && handles_endings;
        if !holds {
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

    /// What a call into the guest's code reads and adds to, made before the turn's records are handed in: the table,
    /// the seats its children are given, and the puts of its blobs.
    pub fn lent(&mut self) -> (&'a dyn Table, &mut Seats, &mut Puts) {
        let (seats, puts, _) = self.seat.asked.lent();

        (self.seat.table, seats, puts)
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

    /// Ends each of `with` but this actor, the actors whose code ran in the instance a call of this turn broke, as a
    /// failed turn of its own would end it, for why it `broke`: what the call left in the memory they live in cannot be
    /// trusted (ADR-0015). The engine keeps what this asks for when the turn fails, as it keeps the turn's lines.
    pub fn broken(&mut self, broke: Broke, with: impl IntoIterator<Item = Seated>) {
        let fault = broke.fault();
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
            Target::Watched => false,
        }
    }
}
