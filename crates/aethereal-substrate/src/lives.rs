//! The view of how far each actor's life has gone: the mail it has sent, and whether it has ended (ADR-0009,
//! ADR-0011).

use crate::id::spawned;
use crate::{ByActor, Context, View};
use aethereal_actor::{Effect, How, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::mem;

/// The view of how far each actor's life has gone, keyed by the actor's own id (ADR-0011).
///
/// A key has one owner. An actor's entry is made by the spawn its parent
/// asked for, or for a root by the chassis, and everything after is the
/// actor's own: the mail it sends, its ending, and its closing turn, which
/// takes the entry out. The actors that end in a step are kept in a list,
/// for the engine to run their closing turns in the next.
pub struct Lives;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// Each actor's life, from when it is made to when its closing turn has run.
    lives: ByActor<Life>,
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
    /// It has ended, `how` it ended, and its closing turn runs in the step after, in `context`.
    Ending { how: How, context: Context },
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor was made.
    Made,
    /// The actor sent the piece of mail it counts as this.
    Sent(u64),
    /// The actor, which sits in `seat`, ended this way, in this context.
    Ended { how: How, context: Context, seat: SeatNumber },
    /// The actor's closing turn has run, and its life is over.
    Over,
}

impl Shard {
    /// The life of the actor whose bits these are, when it is made and its closing turn has not run.
    #[must_use]
    pub fn life(&self, bits: u64) -> Option<&Life> {
        self.lives.get(&bits)
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

    fn fold(shard: &mut Shard, key: u64, record: Record) {
        match record {
            Record::Made => {
                shard.lives.entry(key).or_insert(Life { sent: 0, state: State::Live });
            }
            Record::Sent(count) => {
                if let Some(life) = shard.lives.get_mut(&key) {
                    life.sent = life.sent.max(count);
                }
            }
            Record::Ended { how, context, seat } => {
                if let Some(life) = shard.lives.get_mut(&key) {
                    if life.is_live() {
                        shard.ended.push(Seated { id: ActorId::new(key), seat });
                    }
                    life.state = State::Ending { how, context };
                }
            }
            Record::Over => {
                shard.lives.remove(&key);
            }
        }
    }
}

/// The key and record the view keeps of `effect`, emitted in `context`, or none for an effect it does not fold.
fn record(effect: &Effect, context: &Context) -> Option<(u64, Record)> {
    let Seated { id: by, seat } = effect.seated();
    let (id, record) = match effect.what() {
        What::Root { .. } | What::RootInstanced { .. } => (by, Record::Made),
        What::Send { id, .. } => (id.sender, Record::Sent(id.count)),
        What::Close => (by, Record::Ended { how: How::Closed, context: *context, seat }),
        What::Failed(fault) => (by, Record::Ended { how: How::Failed(fault.clone()), context: *context, seat }),
        What::Ended(_) => (by, Record::Over),
        _ => (spawned(effect)?.0.id, Record::Made),
    };

    Some((id.bits(), record))
}
