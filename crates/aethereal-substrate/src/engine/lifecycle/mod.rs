//! The program's lifecycle, which the engine runs between steps: each stage sent to the actors that subscribe to it,
//! and the next sent only once every turn the last set off has run (ADR-0020).
//!
//! A stage reaches each subscriber as mail from that actor to itself, as what
//! a source brings in does (ADR-0010), and every piece of one stage's mail has
//! one root, so a stage is one chain, however far its mail goes. Whether it
//! has finished is read between steps from what the engine still holds of it
//! (`Engine::has_finished`).

#[cfg(test)]
mod tests;

use super::{Engine, ROOTS, parity};
use crate::id::root_id;
use crate::lives;
use crate::stages::{self, key};
use aethereal_actor::{MailId, Mailed, Stage, Start, Stop, Tie};
use aethereal_data::{ActorId, KindId, KindName, PathSegment};

/// Where the program's lifecycle stands (ADR-0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The host has not started it: no stage has been sent.
    Waiting,
    /// It has started, and nothing has asked it to stop.
    Running,
    /// The host or an actor asked it to stop: `Stop` is sent, or will be once a stage under way has finished.
    Stopping,
    /// `Stop` has finished: the host drops the engine.
    Stopped,
}

/// What the engine keeps of its lifecycle between steps.
pub(super) struct Staging {
    lifecycle: Lifecycle,
    under_way: UnderWay,
    /// How many stages have been sent: the number of the last one's chain.
    sent: u64,
}

/// The stage whose chain the engine waits on, if any.
#[derive(Clone, Copy)]
enum UnderWay {
    /// None: the lifecycle waits on the host, or has stopped.
    Nothing,
    /// A stage, which is `Stop` when `stops`, whose chain begins at `root`.
    Stage { root: MailId, stops: bool },
}

impl Staging {
    /// A lifecycle the host has not started.
    pub(super) const fn new() -> Self {
        Self { lifecycle: Lifecycle::Waiting, under_way: UnderWay::Nothing, sent: 0 }
    }
}

/// The name stages are sent under, which the engine keeps for them: no root is made at it, so no actor has the id
/// their chains begin at.
pub(super) const LIFECYCLE: KindName = KindName::new("aether.lifecycle");

/// The id every stage's chain begins at.
fn lifecycle_id() -> ActorId {
    root_id(&PathSegment::new(LIFECYCLE, &()))
}

impl Engine {
    /// Starts the program's lifecycle, for the host, once it has made its roots: sends `Start` to every actor
    /// subscribed to it by then (ADR-0020). It does nothing once the lifecycle has started.
    pub fn start(&mut self) {
        let is_waiting = self.staging.lifecycle == Lifecycle::Waiting;
        if !is_waiting {
            return;
        }
        self.staging.lifecycle = Lifecycle::Running;
        self.send_stage::<Start>(false);
    }

    /// Asks the program to stop, for the host: when its window closed, the process was told to end, or the page was
    /// hidden. The engine lets a stage under way finish, sends `Stop`, and once that has finished its lifecycle is
    /// [`Lifecycle::Stopped`] (ADR-0020). Asking again changes nothing.
    pub fn quit(&mut self) {
        self.stopping();
        self.advance();
    }

    /// Where the program's lifecycle stands.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.staging.lifecycle
    }

    /// Moves the lifecycle on between steps: past a stage whose chain has finished, and to `Stop` once it is asked
    /// for and nothing is under way. Run after each step's fold.
    pub(super) fn advance(&mut self) {
        // A part's request left untaken here is taken at the next look, and asking twice changes nothing.
        let asked_to_stop = self.stages.shards_mut().any(stages::Shard::take_quit);
        if asked_to_stop {
            self.stopping();
        }
        if let UnderWay::Stage { root, stops } = self.staging.under_way {
            if !self.has_finished(root) {
                return;
            }
            self.staging.under_way = UnderWay::Nothing;
            if stops {
                self.staging.lifecycle = Lifecycle::Stopped;
                return;
            }
        }
        let stops_next = self.staging.lifecycle == Lifecycle::Stopping;
        if stops_next {
            self.send_stage::<Stop>(true);
        }
    }

    /// Marks the lifecycle as asked to stop, unless it has stopped already.
    fn stopping(&mut self) {
        let is_live = matches!(self.staging.lifecycle, Lifecycle::Waiting | Lifecycle::Running);
        if is_live {
            self.staging.lifecycle = Lifecycle::Stopping;
        }
    }

    /// Sends the stage `S` to every actor subscribed to it, as mail from each to itself, in the order of their ids,
    /// all of one chain, and waits on that chain. `stops` says the stage is `Stop`. An actor that has gone is passed
    /// over and taken out of the subscribers; one that has ended and is still to run its closing turn is sent the
    /// stage, which is dropped where it would be handed to it, as all mail to an actor that has ended is.
    fn send_stage<S: Stage>(&mut self, stops: bool) {
        self.staging.sent += 1;
        let root = MailId { sender: lifecycle_id(), count: self.staging.sent };
        self.staging.under_way = UnderWay::Stage { root, stops };

        let kind = KindId::of::<S>();
        let subscribers: Vec<_> = self.stages.shard(key(kind)).subscribers(kind).collect();
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        for subscriber in subscribers {
            let sent = self.lives.shard(subscriber.seat.number()).life(subscriber).map(lives::Life::sent);
            let Some(sent) = sent else {
                self.stages.shard_mut(key(kind)).leave(kind, subscriber);
                continue;
            };
            let Ok(bytes) = arena.write_mail(&S::default()) else {
                continue;
            };
            let count = sent + 1;
            self.lives.fold_records([lives::sent(subscriber, count)], &[]);
            mailed.push(Mailed { by: subscriber, to: subscriber, count, kind, bytes, tie: Tie::TELL });
        }
        self.arenas[parity][ROOTS] = arena;
        self.fold_brought(mailed, Vec::new(), parity, |_| root);
    }
}
