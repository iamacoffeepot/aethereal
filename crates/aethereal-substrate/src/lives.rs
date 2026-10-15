//! The view of how far each actor's life has gone: the mail it has sent, and whether it has ended (ADR-0009,
//! ADR-0011).

use crate::id::spawned;
use crate::{BySeat, Context, Part, View};
use aethereal_actor::{Effect, How, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::mem;
use std::num::NonZeroUsize;

/// The view of how far each actor's life has gone, keyed by the actor's seat (ADR-0011).
///
/// A key has one owner. An actor's entry is made by the spawn its parent
/// asked for, or for a root by the chassis, and everything after is the
/// actor's own: the mail it sends, its ending, and its closing turn, which
/// takes the entry out. The actors that end in a step are kept in a list,
/// for the engine to run their closing turns in the next.
pub struct Lives;

/// One part of the view.
pub struct Shard {
    /// Each actor's life, from when it is made to when its closing turn has run.
    lives: BySeat<Life>,
    /// The actors that ended since the step began, and their seats, in the order they ended.
    ended: Vec<Seated>,
}

/// How far one actor's life has gone.
pub struct Life {
    /// How many pieces of mail it has sent.
    sent: u64,
    /// Whether it still reacts.
    state: State,
}

/// Whether an actor still reacts.
pub enum State {
    /// It reacts to its stream.
    Live,
    /// It has ended: how it ended, and the context its closing turn runs in, in the step after. Kept apart from the
    /// view's list, which holds a life for every actor, since few have ended.
    Ending(Box<(How, Context)>),
}

/// What the view keeps of an effect, for the actor `id` that sits in the key's seat.
pub struct Record {
    id: ActorId,
    change: Change,
}

/// How an effect changes an actor's life.
enum Change {
    /// The actor was made.
    Made,
    /// The actor sent the piece of mail it counts as this.
    Sent(u64),
    /// The actor ended this way, in this context.
    Ended(Box<(How, Context)>),
    /// The actor's closing turn has run, and its life is over.
    Over,
}

impl Shard {
    /// The life of `actor`, when it is made and its closing turn has not run.
    #[must_use]
    pub fn life(&self, actor: Seated) -> Option<&Life> {
        self.lives.get(actor)
    }

    /// Takes the actors that ended since the step began, and their seats, in the order they ended.
    pub fn take_ended(&mut self) -> Vec<Seated> {
        mem::take(&mut self.ended)
    }

    /// Whether an actor has ended since the step began.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        !self.ended.is_empty()
    }
}

impl Part for Shard {
    fn empty(parts: NonZeroUsize) -> Self {
        Self { lives: BySeat::empty(parts), ended: Vec::new() }
    }
}

impl Life {
    /// How many pieces of mail the actor has sent.
    #[must_use]
    pub const fn sent(&self) -> u64 {
        self.sent
    }

    /// Whether the actor still reacts.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Whether the actor reacts to its stream.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self.state, State::Live)
    }
}

impl View for Lives {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        record(effect, context).into_iter()
    }

    fn fold(shard: &mut Shard, key: u64, Record { id, change }: Record) {
        let actor = Seated { id, seat: SeatNumber::new(key) };
        match change {
            Change::Made => {
                shard.lives.entry(actor, || Life { sent: 0, state: State::Live });
            }
            Change::Sent(count) => {
                if let Some(life) = shard.lives.get_mut(actor) {
                    life.sent = life.sent.max(count);
                }
            }
            Change::Ended(ending) => {
                if let Some(life) = shard.lives.get_mut(actor) {
                    if life.is_live() {
                        shard.ended.push(actor);
                    }
                    life.state = State::Ending(ending);
                }
            }
            Change::Over => shard.lives.remove(actor),
        }
    }
}

/// The key and record the view keeps of `effect`, emitted in `context`, or none for an effect it does not fold.
fn record(effect: &Effect, context: &Context) -> Option<(u64, Record)> {
    let by = effect.seated();
    let (actor, change) = match effect.what() {
        What::Root { .. } | What::RootInstanced { .. } => (by, Change::Made),
        What::Send { id, .. } => (by, Change::Sent(id.count)),
        What::Close => (by, Change::Ended(Box::new((How::Closed, *context)))),
        What::Failed(fault) => (by, Change::Ended(Box::new((How::Failed(fault.clone()), *context)))),
        What::Ended(_) => (by, Change::Over),
        _ => (spawned(effect)?.0, Change::Made),
    };

    Some((actor.seat.number(), Record { id: actor.id, change }))
}
