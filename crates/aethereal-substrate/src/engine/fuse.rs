//! Chains of mail run within a step: what one part keeps while it fuses (ADR-0013).

use crate::ByActor;
use aethereal_actor::{Declared, MailId, SeatNumber, Seated};
use aethereal_data::{ActorId, ActorPath, KindId};

/// What one part keeps of its step while it runs it: the hop waiting, the budget left, what each actor that ran has
/// done, the children stood up, and its run (ADR-0013).
///
/// A worker keeps one for every part it runs, emptied as each part begins, so nothing in it is allocated again.
#[derive(Default)]
pub(super) struct Fusing {
    /// The hops the part may still run in this step.
    budget: usize,
    /// The hop waiting to run: never more than one, so a chain fuses and a burst spreads.
    waiting: Vec<Hop>,
    /// The bytes of the hop waiting, copied out of the arena its sender wrote, which the next turn writes on.
    pub(super) bytes: Vec<u8>,
    /// What each actor that ran in the part has done in the step, under its id.
    actors: ByActor<Stepped>,
    /// The paths and seats of the children stood up in the part in the step, and what their types declare, under
    /// their ids.
    pub(super) stood: ByActor<(ActorPath, SeatNumber, &'static Declared)>,
    /// The spawns and endings of the part's run.
    pub(super) run: Run,
    /// How the sends of the parts it ran fared, since the engine last took them in.
    pub(super) hops: Hops,
}

/// How the sends made while a part could still fuse fared: how many were fused, and, for each other, the rule that
/// sent it out as mail (ADR-0013). Sends made once a part's budget is spent are not counted, nor any when the engine
/// fuses nothing.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hops {
    /// Sends fused.
    pub fused: u64,
    /// First sends of a turn made while a hop was waiting: a part runs one run in a step.
    pub waiting: u64,
    /// First sends by an actor that had sent mail as mail to an actor in its part in the step.
    pub spilled: u64,
    /// First sends to a child stood up in the step, which is seated when the step is applied.
    pub unseated: u64,
    /// First sends to an actor in another part, or to one that is gone.
    pub elsewhere: u64,
    /// First sends to an actor that has ended.
    pub ended: u64,
    /// Sends after the first of their turn: a turn fuses at most one.
    pub burst: u64,
    /// Sends of closing turns, which never fuse.
    pub closing: u64,
}

impl Hops {
    /// Adds `other` to these.
    pub(super) fn add(&mut self, other: Self) {
        self.fused += other.fused;
        self.waiting += other.waiting;
        self.spilled += other.spilled;
        self.unseated += other.unseated;
        self.elsewhere += other.elsewhere;
        self.ended += other.ended;
        self.burst += other.burst;
        self.closing += other.closing;
    }

    /// Counts a first send that `hand` handled.
    pub(super) fn count(&mut self, hand: Hand) {
        let counter = match hand {
            Hand::Fuse => &mut self.fused,
            Hand::Waiting => &mut self.waiting,
            Hand::Spilled => &mut self.spilled,
            Hand::Unseated => &mut self.unseated,
            Hand::Elsewhere => &mut self.elsewhere,
            Hand::Ended => &mut self.ended,
        };
        *counter += 1;
    }
}

/// What becomes of a turn's first send while its part may still fuse (ADR-0013).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hand {
    /// It is fused.
    Fuse,
    /// It goes out as mail: a hop is waiting.
    Waiting,
    /// It goes out as mail: its sender has sent mail as mail to an actor in its part in the step.
    Spilled,
    /// It goes out as mail: its receiver was stood up in the step and has no seat yet.
    Unseated,
    /// It goes out as mail: its receiver sits in another part, or is gone.
    Elsewhere,
    /// It goes out as mail, to be dropped: its receiver has ended.
    Ended,
}

/// One piece of mail handed straight to its receiver, in the same part, to run in the same step (ADR-0013).
#[derive(Clone, Copy)]
pub(super) struct Hop {
    /// The receiver, and its seat.
    pub(super) to: Seated,
    /// The sender, and its seat.
    pub(super) from: Seated,
    /// The mail's id.
    pub(super) id: MailId,
    /// The first mail of its chain.
    pub(super) root: MailId,
    /// How far down its chain it is.
    pub(super) depth: u32,
    pub(super) kind: KindId,
}

/// What one actor has done in a step, in its part (ADR-0013).
#[derive(Clone, Copy)]
pub(super) struct Stepped {
    /// The count of the last mail it sent.
    pub(super) sent: u64,
    /// Whether it has sent mail as mail to an actor in its part: it then fuses nothing more in the step, so a hop
    /// never overtakes mail from the same sender to the same receiver (ADR-0009).
    pub(super) spilled: bool,
    /// Whether it has ended: it handles nothing more, so a hop to it is dropped.
    pub(super) ended: bool,
}

/// What the turns of one run have done to the table: the children they stood up, and the actors that ended in them
/// (ADR-0013).
///
/// A run is the turn that made a part's first fused send and the turns of the hops that followed. Only its own turns
/// see this, until the step is folded.
#[derive(Default)]
pub(super) struct Run {
    stood: Vec<(Seated, ActorPath)>,
    ended: Vec<ActorId>,
}

impl Fusing {
    /// Empties what the last part left, for a part that may run `budget` hops.
    pub(super) fn begin(&mut self, budget: usize) {
        self.budget = budget;
        self.waiting.clear();
        self.actors.clear();
        self.stood.clear();
        self.run.stood.clear();
        self.run.ended.clear();
    }

    /// Whether the part takes a hop now: one is not waiting, and its budget has room.
    pub(super) fn takes_a_hop(&self) -> bool {
        let is_free = self.waiting.is_empty();

        is_free && self.budget > 0
    }

    /// Whether the part may still make a hop in this step: its budget has room.
    pub(super) const fn may_hop(&self) -> bool {
        self.budget > 0
    }

    /// Whether the part may still run a hop in this step: one is waiting, or it may still make one. While it may, what
    /// each actor does is kept, for a hop to it to go on from.
    pub(super) fn is_open(&self) -> bool {
        self.may_hop() || !self.waiting.is_empty()
    }

    /// Puts `hop` waiting, spending one hop of the budget.
    pub(super) fn wait(&mut self, hop: Hop) {
        self.budget -= 1;
        self.waiting.push(hop);
    }

    /// The hop waiting, taken to run.
    pub(super) fn next(&mut self) -> Option<Hop> {
        self.waiting.pop()
    }

    /// What the actor `me` names has done in the step, or, when it has not run, what it had done when the step began:
    /// `sent` pieces of mail.
    pub(super) fn stepped(&self, me: Seated, sent: u64) -> Stepped {
        let fresh = Stepped { sent, spilled: false, ended: false };

        self.actors.get(&me.id.bits()).copied().unwrap_or(fresh)
    }

    /// Keeps what the actor `me` names has done in the step.
    pub(super) fn keep(&mut self, me: Seated, stepped: Stepped) {
        self.actors.insert(me.id.bits(), stepped);
    }
}

impl Run {
    /// Notes the children `stood` that a turn of the run stood up.
    pub(super) fn stand(&mut self, stood: impl IntoIterator<Item = (Seated, ActorPath)>) {
        self.stood.extend(stood);
    }

    /// Notes that the actor `id` stands for ended in the run.
    pub(super) fn end(&mut self, id: ActorId) {
        self.ended.push(id);
    }

    /// The seat of the child the run stood up under `id`, when it stood one up.
    pub(super) fn stood(&self, id: ActorId) -> Option<SeatNumber> {
        self.stood.iter().find(|(child, _)| child.id == id).map(|(child, _)| child.seat)
    }

    /// The child the run stood up at `path`, and its seat, when it stood one up.
    pub(super) fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        self.stood.iter().find(|(_, at)| at == path).map(|(child, _)| *child)
    }

    /// Whether the actor `id` stands for ended in the run.
    pub(super) fn has_ended(&self, id: ActorId) -> bool {
        self.ended.contains(&id)
    }
}
