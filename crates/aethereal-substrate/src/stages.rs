//! The view of which actors subscribe to each stage of the program's lifecycle, and whether one asked it to stop
//! (ADR-0011, ADR-0020).

use crate::{Context, View};
use aethereal_actor::{Arena, Effect, Seated, What};
use aethereal_data::KindId;
use std::collections::BTreeMap;
use std::mem;

/// The view of which actors subscribe to each stage, keyed by the stage's kind (ADR-0020).
///
/// Many actors write one stage's key, and a set is the same in whatever
/// order its records arrive (ADR-0011). An actor that ends is not taken out
/// when it ends: the engine passes over it, and takes it out, when it next
/// sends the stage, as it finds it no longer live. A request to stop is kept
/// under a key of its own, which many actors write too, and which only
/// becomes true.
pub struct Stages;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// The actors subscribed to each stage, under the stage's key: by their ids' bits, in that order.
    subscribed: BTreeMap<u64, BTreeMap<u64, Seated>>,
    /// Whether an actor whose request to stop falls in this part has asked since the engine last looked.
    quit: bool,
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor subscribes to the key's stage.
    Subscribe(Seated),
    /// The actor no longer subscribes to the key's stage.
    Unsubscribe(Seated),
    /// An actor asked the program to stop.
    Quit,
}

/// The key a request to stop is kept under: no stage's, as a stage's kind has a name.
const QUIT: u64 = 0;

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

    /// Whether an actor asked the program to stop since this was last asked.
    pub fn take_quit(&mut self) -> bool {
        mem::take(&mut self.quit)
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
            What::Quit => Some((QUIT, Record::Quit)),
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
            Record::Quit => shard.quit = true,
        }
    }
}
