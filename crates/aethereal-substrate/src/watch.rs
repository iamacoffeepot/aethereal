//! The view of who watches each actor, and the endings a step shows its watchers (ADR-0009, ADR-0011).

use crate::{Folded, View};
use aethereal_actor::{Effect, Ended, How, What};
use aethereal_data::ActorId;
use std::collections::{BTreeMap, BTreeSet};

/// The view of who watches each actor, keyed by the id of the actor watched (ADR-0011).
///
/// Many actors write one key, each adding itself, and a set comes out the
/// same in whatever order they do. The watched actor's own ending closes its
/// key: its watchers are dropped, and a watch asked for after that adds
/// nothing.
pub struct Watchers;

/// One watched actor's entry.
pub enum Watched {
    /// The actor has not ended, and these actors watch it.
    By(BTreeSet<u64>),
    /// The actor has ended.
    Ended,
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor `by` stands for watches the key's actor.
    Watch { by: ActorId },
    /// The key's actor ended.
    Ended,
}

impl View for Watchers {
    type Shard = BTreeMap<u64, Watched>;
    type Record = Record;

    fn takes(effect: &Effect) -> Option<(u64, Record)> {
        let by = effect.by();
        match effect.what() {
            What::Watch { of } => Some((of.id().bits(), Record::Watch { by })),
            What::Failed(_) | What::Close => Some((by.bits(), Record::Ended)),
            _ => None,
        }
    }

    fn fold(shard: &mut BTreeMap<u64, Watched>, key: u64, record: Record) {
        match record {
            Record::Watch { by } => {
                if let Watched::By(watchers) = shard.entry(key).or_insert_with(|| Watched::By(BTreeSet::new())) {
                    watchers.insert(by.bits());
                }
            }
            Record::Ended => {
                shard.insert(key, Watched::Ended);
            }
        }
    }
}

/// The endings one step shows, each watcher's under its id and in the order of the ended actors' ids, each with
/// the tag of the effect it came from.
///
/// `watchers` is the view before the step's effects are folded into it, and `effects` are those effects, each
/// with a tag: the engine's is the chain of the turn that asked for it. A watcher is shown an ending when it
/// watched the actor before the step, or asked to in the step, and the ending carries the tag of the effect that
/// ended the actor. A watch asked for on an actor that ended in an earlier step is shown that ending as
/// [`How::Earlier`], with the tag of the watch.
pub fn shown<'a, T: Copy>(
    watchers: &Folded<Watchers>,
    effects: impl Iterator<Item = (&'a Effect, T)> + Clone,
) -> BTreeMap<u64, Vec<(Ended, T)>> {
    let ends: BTreeMap<u64, (How, T)> =
        effects.clone().filter_map(|(effect, tag)| Some((effect.by().bits(), (how(effect)?, tag)))).collect();

    let mut shown: BTreeMap<u64, BTreeMap<u64, (How, T)>> = BTreeMap::new();
    for (ended, (how, tag)) in &ends {
        if let Some(Watched::By(before)) = watchers.shard(*ended).get(ended) {
            for watcher in before {
                shown.entry(*watcher).or_default().insert(*ended, (how.clone(), *tag));
            }
        }
    }
    for (effect, tag) in effects {
        let What::Watch { of } = effect.what() else {
            continue;
        };
        let watched = of.id().bits();
        let ended_earlier = matches!(watchers.shard(watched).get(&watched), Some(Watched::Ended));
        let shown_as = match ends.get(&watched) {
            Some((how, ended_by)) => (how.clone(), *ended_by),
            None if ended_earlier => (How::Earlier, tag),
            None => continue,
        };
        shown.entry(effect.by().bits()).or_default().insert(watched, shown_as);
    }

    shown
        .into_iter()
        .map(|(watcher, ended)| {
            let ended =
                ended.into_iter().map(|(actor, (how, tag))| (Ended::new(ActorId::new(actor), how), tag)).collect();

            (watcher, ended)
        })
        .collect()
}

/// How the actor an effect is by ended, when the effect is an ending.
fn how(effect: &Effect) -> Option<How> {
    match effect.what() {
        What::Close => Some(How::Closed),
        What::Failed(fault) => Some(How::Failed(fault.clone())),
        _ => None,
    }
}
