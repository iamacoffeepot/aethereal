//! The view of what each actor logged most recently (ADR-0009, ADR-0011).

use crate::{BySeat, Context, View};
use aethereal_actor::{Arena, Effect, Fault, Level, Line, Payload, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::collections::VecDeque;

/// How many entries the view keeps for one actor. An older one leaves as a newer one arrives.
pub const RECENT: usize = 32;

/// The view of what each actor logged most recently, keyed by the actor's seat (ADR-0011).
///
/// A key has one owner: an entry is by the actor it is kept under, so one
/// actor's entries arrive in the order it made them. An actor's entries stay
/// once it has ended, since that is when why it ended is read, until another
/// actor takes its seat and logs: a log worth keeping longer is copied out
/// before then.
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

/// What the view keeps of a line or a fault before it is folded: a line's text is still where its turn wrote it.
pub enum Logged {
    /// A line, its text at `text` in the arena of the worker that ran its turn.
    Line { level: Level, text: Payload },
    /// A turn of the actor's returned this fault, and the actor ended.
    Failed(Fault),
    /// The actor's closing turn returned this fault.
    EndFailed(Fault),
}

impl View for Log {
    type Shard = BySeat<VecDeque<Entry>>;
    /// A line or a fault, and the actor in the key's seat that made it.
    type Record = (ActorId, Logged);

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, (ActorId, Logged))> + 'a {
        fault(effect).into_iter()
    }

    /// A line's text is copied out of its arena into the actor's entries, into the text of the entry that leaves as
    /// it arrives, so a log kept full allocates nothing.
    fn fold(shard: &mut BySeat<VecDeque<Entry>>, key: u64, (id, logged): (ActorId, Logged), written: &[Arena]) {
        let recent = shard.entry(Seated { id, seat: SeatNumber::new(key) }, VecDeque::new);
        let is_full = recent.len() == RECENT;
        let left = if is_full {
            recent.pop_front()
        } else {
            None
        };
        let entry = match logged {
            Logged::Line { level, text } => {
                let mut kept = match left {
                    Some(Entry::Line { text, .. }) => text,
                    _ => String::new(),
                };
                kept.clear();
                let bytes = written.get(text.slot()).and_then(|arena| arena.read(text)).unwrap_or_default();
                // The turn wrote the text through `fmt::Write`, so it is whole UTF-8.
                kept.push_str(str::from_utf8(bytes).unwrap_or_default());

                Entry::Line { level, text: kept }
            }
            Logged::Failed(fault) => Entry::Failed(fault),
            Logged::EndFailed(fault) => Entry::EndFailed(fault),
        };
        recent.push_back(entry);
    }
}

/// The key and record the view keeps of `line`: the line, under its actor's seat.
#[must_use]
pub fn line(line: Line) -> (u64, (ActorId, Logged)) {
    let Line { by, level, text } = line;

    (by.seat.number(), (by.id, Logged::Line { level, text }))
}

/// The key and record the view keeps of `effect`, a fault, or none for an effect it does not keep.
fn fault(effect: &Effect) -> Option<(u64, (ActorId, Logged))> {
    let logged = match effect.what() {
        What::Failed(fault) => Logged::Failed(fault.clone()),
        What::EndFailed(fault) => Logged::EndFailed(fault.clone()),
        _ => return None,
    };
    let by = effect.seated();

    Some((by.seat.number(), (by.id, logged)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Folded;
    use std::num::NonZeroUsize;

    #[test]
    fn oldest_entry_leaves_when_an_actor_logs_past_the_most_kept() {
        // Catches a log that grows without end, one that drops the newest entry or another actor's, and an entry
        // that leaves keeping its text in the one that arrives.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let loud = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let quiet = Seated { id: ActorId::new(2), seat: SeatNumber::new(5) };
        let mut arena = Arena::new(0);
        let mut logs = |by, number: usize| {
            let Ok(text) = arena.write_text(number) else {
                panic!("a number shows");
            };

            line(Line { by, level: Level::Info, text })
        };
        let mut lines = vec![logs(quiet, 0)];
        lines.extend((0..=RECENT).map(|number| logs(loud, number)));
        let mut log = Folded::<Log>::new(parts);
        log.fold_records(lines, &[arena]);

        let texts = |by: Seated| -> Vec<String> {
            let kept = log.shard(by.seat.number()).get(by).into_iter().flatten();

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
