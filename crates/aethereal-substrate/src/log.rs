//! The view of what each actor logged most recently (ADR-0009, ADR-0011).

use crate::{Context, View};
use aethereal_actor::{Effect, Fault, Level, What};
use std::collections::{BTreeMap, VecDeque};

/// How many entries the view keeps for one actor. An older one leaves as a newer one arrives.
pub const RECENT: usize = 32;

/// The view of what each actor logged most recently, keyed by the actor's own id (ADR-0011).
///
/// A key has one owner: an entry is by the actor it is kept under, so one
/// actor's entries arrive in the order it made them. An actor's entries stay
/// once it has ended, since that is when why it ended is read.
pub struct Log;

/// One entry in an actor's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A line the actor logged.
    Line { level: Level, text: String },
    /// A turn of the actor's returned this fault, and the actor ended.
    Failed(Fault),
    /// The actor's closing turn returned this fault.
    EndFailed(Fault),
}

impl View for Log {
    type Shard = BTreeMap<u64, VecDeque<Entry>>;
    type Record = Entry;
    type Reads<'a> = ();

    fn takes<'a>(effect: &'a Effect, _: &'a Context, (): ()) -> impl Iterator<Item = (u64, Entry)> + 'a {
        entry(effect).into_iter()
    }

    fn fold(shard: &mut BTreeMap<u64, VecDeque<Entry>>, key: u64, entry: Entry) {
        let recent = shard.entry(key).or_default();
        let is_full = recent.len() == RECENT;
        if is_full {
            recent.pop_front();
        }
        recent.push_back(entry);
    }
}

/// The key and entry the view keeps of `effect`, or none for an effect it does not keep.
fn entry(effect: &Effect) -> Option<(u64, Entry)> {
    let entry = match effect.what() {
        What::Log { level, text } => Entry::Line { level: *level, text: text.clone() },
        What::Failed(fault) => Entry::Failed(fault.clone()),
        What::EndFailed(fault) => Entry::EndFailed(fault.clone()),
        _ => return None,
    };

    Some((effect.by().bits(), entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Folded;
    use aethereal_actor::MailId;
    use aethereal_data::ActorId;
    use std::num::NonZeroUsize;

    #[test]
    fn oldest_entry_leaves_when_an_actor_logs_past_the_most_kept() {
        // Catches a log that grows without end, and one that drops the newest entry or another actor's.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (loud, quiet) = (ActorId::new(1), ActorId::new(2));
        let line = |by, number: usize| Effect::new(by, What::Log { level: Level::Info, text: number.to_string() });
        let mut effects = vec![line(quiet, 0)];
        effects.extend((0..=RECENT).map(|number| line(loud, number)));
        let mut log = Folded::<Log>::new(parts);
        let context = Context { cause: None, root: MailId { sender: loud, count: 0 } };
        log.fold(&effects, &context, ());

        let texts = |by: ActorId| -> Vec<String> {
            let kept = log.shard(by.bits()).get(&by.bits()).into_iter().flatten();

            kept.filter_map(|entry| {
                if let Entry::Line { text, .. } = entry {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect()
        };
        assert_eq!(texts(loud), (1..=RECENT).map(|number| number.to_string()).collect::<Vec<_>>());
        assert_eq!(texts(quiet), ["0"]);
    }
}
