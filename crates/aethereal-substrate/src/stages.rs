//! The view of which actors subscribe to each stage of the program's lifecycle, and which asked it to stop or failed
//! a turn, which the lifecycle reads to stop the program or fail its boot (ADR-0011, ADR-0020).

use crate::{Context, View};
use aethereal_actor::{Arena, Effect, Fault, Seated, What};
use aethereal_data::KindId;
use std::collections::BTreeMap;

/// The view of which actors subscribe to each stage, keyed by the stage's kind (ADR-0020).
///
/// Many actors write one stage's key, and a set is the same in whatever
/// order its records arrive (ADR-0011). An actor that ends is not taken out
/// when it ends: the engine passes over it, and takes it out, when it next
/// sends the stage, as it finds it no longer live. A request to stop, and a
/// turn that failed, are kept under a key of their own, which many actors
/// write too, until the lifecycle takes them.
pub struct Stages;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// The actors subscribed to each stage, under the stage's key: by their ids' bits, in that order.
    subscribed: BTreeMap<u64, BTreeMap<u64, Seated>>,
    /// What actors asked of the lifecycle since the engine last looked, a quit or a turn that failed, in the order
    /// folded.
    halts: Vec<Halt>,
}

/// What an actor did that the lifecycle reads: asked the program to stop, or failed a turn (ADR-0020). Once the
/// program has started a quit stops it, and so does a root's failure; while it boots, either fails the boot.
#[derive(Debug, Clone)]
pub enum Halt {
    /// The actor asked the program to stop.
    Quit(Seated),
    /// A turn of the actor failed, with the fault.
    Failed(Seated, Fault),
}

impl Halt {
    /// The actor that halted.
    #[must_use]
    pub const fn actor(&self) -> Seated {
        match self {
            Self::Quit(actor) | Self::Failed(actor, _) => *actor,
        }
    }

    /// Whether the actor asked the program to stop.
    #[must_use]
    pub const fn is_quit(&self) -> bool {
        matches!(self, Self::Quit(_))
    }

    /// The actor whose turn failed, and the fault, when it is a failure.
    #[must_use]
    pub fn failure(self) -> Option<(Seated, Fault)> {
        match self {
            Self::Quit(_) => None,
            Self::Failed(actor, fault) => Some((actor, fault)),
        }
    }

    /// Where it stands among the halts of one look: by its actor's id, so which fails a boot does not hang on the
    /// order a step's turns ran in.
    #[must_use]
    pub const fn order(&self) -> u64 {
        self.actor().id.bits()
    }
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor subscribes to the key's stage.
    Subscribe(Seated),
    /// The actor no longer subscribes to the key's stage.
    Unsubscribe(Seated),
    /// An actor asked the program to stop, or failed a turn.
    Halt(Halt),
}

/// The key a request to stop and a failed turn are kept under: no stage's, as a stage's kind has a name.
const HALT: u64 = 0;

/// The key the stage of kind `kind` is kept under.
#[must_use]
pub fn key(kind: KindId) -> u64 {
    u64::from_le_bytes(kind.plain_bytes())
}

impl Shard {
    /// The actors subscribed to the stage of kind `kind`, in the order of their ids, as of the last fold.
    pub fn subscribers(&self, kind: KindId) -> impl Iterator<Item = Seated> + '_ {
        self.subscribed.get(&key(kind)).into_iter().flat_map(|subscribed| subscribed.values().copied())
    }

    /// Takes the actor `actor` out of the subscribers of the stage of kind `kind`: one found no longer live.
    pub fn leave(&mut self, kind: KindId, actor: Seated) {
        if let Some(subscribed) = self.subscribed.get_mut(&key(kind)) {
            subscribed.remove(&actor.id.bits());
        }
    }

    /// What actors asked of the lifecycle since this was last asked, a quit or a turn that failed, in the order
    /// folded, taken out.
    pub fn take_halts(&mut self) -> impl Iterator<Item = Halt> + '_ {
        self.halts.drain(..)
    }
}

impl View for Stages {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let by = effect.seated();
        let record = match effect.what() {
            What::Subscribe(kind) => Some((key(*kind), Record::Subscribe(by))),
            What::Unsubscribe(kind) => Some((key(*kind), Record::Unsubscribe(by))),
            What::Quit => Some((HALT, Record::Halt(Halt::Quit(by)))),
            What::Failed(fault) => Some((HALT, Record::Halt(Halt::Failed(by, fault.clone())))),
            _ => None,
        };

        record.into_iter()
    }

    fn fold(shard: &mut Shard, key: u64, record: Record, _: &[Arena]) {
        match record {
            Record::Subscribe(actor) => {
                shard.subscribed.entry(key).or_default().insert(actor.id.bits(), actor);
            }
            Record::Unsubscribe(actor) => {
                if let Some(subscribed) = shard.subscribed.get_mut(&key) {
                    subscribed.remove(&actor.id.bits());
                }
            }
            Record::Halt(halt) => shard.halts.push(halt),
        }
    }
}
