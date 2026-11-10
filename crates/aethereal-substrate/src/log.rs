//! The view of what each actor logged most recently (ADR-0009, ADR-0011), and of what every actor did, across actors
//! (ADR-0053).

use crate::{ByActor, BySeat, Context, Folded, Part, View};
use aethereal_actor::{
    ActorName, Arena, Effect, Entry, Fault, KeptLog, Level, Line, Named, Payload, Recent, RecentRing, Recorded,
    SeatNumber, Seated, Unreadable, What,
};
use aethereal_data::ActorId;
use std::num::NonZeroUsize;

/// How many entries the view keeps for one actor by default, as `EngineSettings::most_logged` sets it (ADR-0044). An
/// older one leaves as a newer one arrives.
pub const MOST_LOGGED: usize = 32;

/// How many of every actor's most recent entries the view keeps between its parts by default, as
/// `EngineSettings::most_recent` sets it (ADR-0053). An older one leaves as a newer one arrives.
pub const MOST_RECENT: usize = 2000;

/// The view of what each actor logged most recently, keyed by the actor's seat (ADR-0011).
///
/// A key has one owner: an entry is by the actor it is kept under, so one
/// actor's entries arrive in the order it made them, and are numbered in that
/// order (ADR-0044). An actor's entries stay once it has ended, since that is
/// when why it ended is read, until another actor takes its seat and logs: a
/// log worth keeping longer is copied out before then.
pub struct Log;

/// One part of the log: the entries of each actor whose seat falls in it, the seat each such actor logged in by its
/// id, and how many each keeps; and the part's most recent entries across those actors, each with the step it was
/// folded in (ADR-0053).
pub struct Logs {
    seats: BySeat<KeptLog>,
    /// The seat each actor kept here logged in, by its id: written as an actor first logs in a seat, so a log is found
    /// by id in one lookup a part, live or ended (ADR-0044).
    ids: ByActor<SeatNumber>,
    /// The most entries an actor keeps: fixed as the engine is set up, before any fold.
    most: usize,
    /// The part's most recent entries, across its actors, filled by the fold that keeps each actor's: no part writes
    /// another's, so the parts still fold apart (ADR-0053).
    recent: RecentRing,
    /// The most entries the part's ring keeps: its share of the view's, fixed as the engine is set up, before any fold.
    most_recent: usize,
    /// The step the part's next entries are folded in: set as each step is planned, before its turns run, so what
    /// is folded between two steps goes with the step before (ADR-0053).
    step: u64,
}

impl Part for Logs {
    fn empty(parts: NonZeroUsize) -> Self {
        Self {
            seats: BySeat::empty(parts),
            ids: ByActor::default(),
            most: MOST_LOGGED,
            recent: RecentRing::default(),
            most_recent: share(MOST_RECENT, parts),
            step: 0,
        }
    }
}

/// One part's share of `most` entries kept between `parts` parts: an equal share, as each part holds an equal share
/// of the seats, rounded up, so the parts together keep at least `most` (ADR-0053).
#[must_use]
pub const fn share(most: usize, parts: NonZeroUsize) -> usize {
    most.div_ceil(parts.get())
}

impl Logs {
    /// What `actor` has logged, when it is the actor kept in its seat.
    #[must_use]
    pub fn get(&self, actor: Seated) -> Option<&KeptLog> {
        self.seats.get(actor)
    }

    /// Keeps at most `most` entries for each actor from here on: for the engine's setup, before any fold.
    pub const fn keep_at_most(&mut self, most: usize) {
        self.most = most;
    }

    /// Keeps at most `most` of the part's most recent entries across its actors from here on: for the engine's setup,
    /// before any fold (ADR-0053).
    pub const fn keep_recent_at_most(&mut self, most: usize) {
        self.most_recent = most;
    }

    /// Folds the part's next entries in the step `step`: for the engine, as it plans each step (ADR-0053).
    pub const fn stamp(&mut self, step: u64) {
        self.step = step;
    }

    /// The part's most recent entries, across its actors.
    #[must_use]
    pub const fn recent(&self) -> &RecentRing {
        &self.recent
    }
}

/// What every part's ring holds from the step `since` on, as [`Recent`] has it, each entry's actor named by `name`
/// once for the read (ADR-0053). The read goes on from the step the parts are stamped with, which a turn reading them
/// runs in, so a reader that goes on from it sees each entry once.
pub fn recent(log: &Folded<Log>, since: u64, mut name: impl FnMut(ActorId) -> ActorName) -> Recent<'_> {
    let parts = log.shards().enumerate();
    let mut found: Vec<(u64, usize, &Recorded)> = parts
        .flat_map(|(part, logs)| logs.recent.since(since).map(move |recorded| (recorded.step(), part, recorded)))
        .collect();
    // A stable sort, so a part's entries within one step stay in the order it folded them.
    found.sort_by_key(|(step, part, _)| (*step, *part));

    let mut names: ByActor<ActorName> = ByActor::default();
    let entries = found
        .into_iter()
        .map(|(_, _, recorded)| {
            let by = recorded.by();
            let actor = names.entry(by.bits()).or_insert_with(|| name(by)).clone();

            Named { actor, recorded }
        })
        .collect();
    let stamped = log.shards().map(|logs| logs.step).max().unwrap_or(since);
    let left = log.shards().map(|logs| logs.recent.left_since(since)).sum();

    Recent { entries, next: stamped.max(since), left }
}

/// What the actor `id` has logged, live or ended, found in the seat it last logged in while no other actor has logged
/// there since: read by id, for a log read by path, in one lookup a part (ADR-0044).
pub fn find(log: &Folded<Log>, id: ActorId) -> Option<&KeptLog> {
    log.shards().find_map(|logs| {
        let seat = *logs.ids.get(&id.bits())?;

        logs.seats.get(Seated { id, seat })
    })
}

/// What the view keeps of a line or a fault before it is folded: a line's text is still where its turn wrote it.
pub enum Logged {
    /// A line, its text at `text` in the arena of the worker that ran its turn.
    Line { level: Level, text: Payload },
    /// A turn of the actor's returned this fault, and the actor ended.
    Failed(Fault),
    /// The actor's closing turn returned this fault.
    EndFailed(Fault),
    /// A resource of this name was dropped on its way.
    Dropped(&'static str),
    /// What a source brought, of the kind this names, could not be written as mail.
    Unbrought(&'static str),
    /// A job the actor ran returned this fault.
    JobFaulted(Fault),
    /// Mail the actor was handed did not read, for this reason.
    Unread(Unreadable),
}

impl View for Log {
    type Shard = Logs;
    /// A line or a fault, and the actor in the key's seat that made it.
    type Record = (ActorId, Logged);

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, (ActorId, Logged))> + 'a {
        fault(effect).into_iter()
    }

    /// A line's text is copied out of its arena into the actor's entries, and from there into the part's ring, each
    /// into the text of the entry that leaves as it arrives, so a log and a ring kept full allocate nothing.
    fn fold(shard: &mut Logs, key: u64, (id, logged): (ActorId, Logged), written: &[Arena]) {
        let Logs { seats, ids, most, recent, most_recent, step } = shard;
        let seated = Seated { id, seat: SeatNumber::new(key) };
        let is_first = seats.get(seated).is_none();
        if is_first {
            // The seat's last actor, if any, leaves the index as this one takes its place in it.
            if let Some(gone) = seats.id_in(seated.seat) {
                ids.remove(&gone.bits());
            }
            ids.insert(id.bits(), seated.seat);
        }
        let kept = seats.entry(seated, KeptLog::default);
        kept.keep(*most, |left| {
            let made = entry(logged, left, written);
            recent.keep(*most_recent, id, *step, |leaving| copied(&made, leaving));

            made
        });
    }
}

/// The text of `left`, an entry that leaves as another arrives, emptied for the one that arrives: a new one when it
/// held none.
fn emptied(left: Option<Entry>) -> String {
    let mut text = match left {
        Some(Entry::Line { text, .. }) => text,
        _ => String::new(),
    };
    text.clear();

    text
}

/// A copy of `entry` for a ring, a line's text copied into the text of `left`, the ring's entry that leaves as it
/// arrives, so a ring kept full allocates nothing for a line.
fn copied(entry: &Entry, left: Option<Entry>) -> Entry {
    let Entry::Line { level, text } = entry else {
        return entry.clone();
    };
    let mut kept = emptied(left);
    kept.push_str(text);

    Entry::Line { level: *level, text: kept }
}

/// The entry `logged` comes to, a line's text copied out of its arena into the text of `left`, the entry that leaves
/// as it arrives, so a log kept full allocates nothing.
fn entry(logged: Logged, left: Option<Entry>, written: &[Arena]) -> Entry {
    match logged {
        Logged::Line { level, text } => {
            let mut kept = emptied(left);
            let bytes = written.get(text.slot()).and_then(|arena| arena.read(text)).unwrap_or_default();
            // The turn wrote the text through `fmt::Write`, so it is whole UTF-8.
            kept.push_str(str::from_utf8(bytes).unwrap_or_default());

            Entry::Line { level, text: kept }
        }
        Logged::Failed(fault) => Entry::Failed(fault),
        Logged::EndFailed(fault) => Entry::EndFailed(fault),
        Logged::Dropped(name) => Entry::Dropped(name),
        Logged::Unbrought(name) => Entry::Unbrought(name),
        Logged::JobFaulted(fault) => Entry::JobFaulted(fault),
        Logged::Unread(reason) => Entry::Unread(reason),
    }
}

/// The key and record the view keeps of `line`: the line, under its actor's seat.
#[must_use]
pub fn line(line: Line) -> (u64, (ActorId, Logged)) {
    let Line { by, level, text } = line;

    (by.seat.number(), (by.id, Logged::Line { level, text }))
}

/// The key and record the view keeps of a resource of the name `name` dropped on its way, under the actor `by`, which
/// gave it or was given it (ADR-0010).
#[must_use]
pub const fn dropped(by: Seated, name: &'static str) -> (u64, (ActorId, Logged)) {
    (by.seat.number(), (by.id, Logged::Dropped(name)))
}

/// The key and record the view keeps of what a source brought, of the kind `name` names, that could not be written as
/// mail, under the actor `by` that owns the source (ADR-0010).
#[must_use]
pub const fn unbrought(by: Seated, name: &'static str) -> (u64, (ActorId, Logged)) {
    (by.seat.number(), (by.id, Logged::Unbrought(name)))
}

/// The key and record the view keeps of the fault a job the actor `by` ran returned (ADR-0018).
#[must_use]
pub const fn job_faulted(by: Seated, fault: Fault) -> (u64, (ActorId, Logged)) {
    (by.seat.number(), (by.id, Logged::JobFaulted(fault)))
}

/// The key and record the view keeps of `effect`, a fault or mail that did not read, or none for an effect it does not
/// keep.
fn fault(effect: &Effect) -> Option<(u64, (ActorId, Logged))> {
    let logged = match effect.what() {
        What::Failed(fault) => Logged::Failed(fault.clone()),
        What::EndFailed(fault) => Logged::EndFailed(fault.clone()),
        What::Unread(reason) => Logged::Unread(*reason),
        _ => return None,
    };
    let by = effect.seated();

    Some((by.seat.number(), (by.id, logged)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;

    /// Two parts, so seats fall in both.
    fn parts() -> NonZeroUsize {
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };

        parts
    }

    /// The record of the line `number` by `by`, written into `arena`.
    fn logs(arena: &mut Arena, by: Seated, number: usize) -> (u64, (ActorId, Logged)) {
        let Ok(text) = arena.write_text(number) else {
            panic!("a number shows");
        };

        line(Line { by, level: Level::Info, text })
    }

    /// The texts of the lines `kept` holds, oldest first.
    fn texts(kept: Option<&KeptLog>) -> Vec<String> {
        let entries = kept.into_iter().flat_map(KeptLog::entries);

        entries
            .filter_map(|entry| match entry {
                Entry::Line { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn oldest_entry_leaves_when_an_actor_logs_past_the_most_kept() {
        // Catches a log that grows without end, one that drops the newest entry or another actor's, an entry that
        // leaves keeping its text in the one that arrives, and numbers that restart or count what is kept, not made.
        let loud = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let quiet = Seated { id: ActorId::new(2), seat: SeatNumber::new(5) };
        let mut arena = Arena::new(0);
        let mut lines = vec![logs(&mut arena, quiet, 0)];
        lines.extend((0..=MOST_LOGGED).map(|number| logs(&mut arena, loud, number)));
        let mut log = Folded::<Log>::new(parts());
        log.fold_records(lines, &[arena]);

        let kept = |by: Seated| log.shard(by.seat.number()).get(by);
        let loud_kept = kept(loud).map(|kept| (kept.first(), kept.next()));
        assert_eq!(texts(kept(loud)), (1..=MOST_LOGGED).map(|number| number.to_string()).collect::<Vec<_>>());
        assert_eq!(loud_kept, Some((1, MOST_LOGGED as u64 + 1)));
        assert_eq!(texts(kept(quiet)), ["0"]);
    }

    #[test]
    fn a_log_set_shorter_keeps_that_many() {
        // Catches the setting ignored, as the constant it replaced was fixed.
        let actor = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let mut arena = Arena::new(0);
        let lines: Vec<_> = (0..5).map(|number| logs(&mut arena, actor, number)).collect();
        let mut log = Folded::<Log>::new(parts());
        log.shards_mut().for_each(|logs| logs.keep_at_most(2));
        log.fold_records(lines, &[arena]);

        assert_eq!(texts(log.shard(actor.seat.number()).get(actor)), ["3", "4"]);
    }

    /// The record of the line `text` by `by`, written into `arena`.
    fn says(arena: &mut Arena, by: Seated, text: &str) -> (u64, (ActorId, Logged)) {
        let Ok(text) = arena.write_text(text) else {
            panic!("a text shows");
        };

        line(Line { by, level: Level::Info, text })
    }

    /// Folds `lines`, each the text said by its actor, into `log` in the step `step`.
    fn fold_in(log: &mut Folded<Log>, step: u64, lines: &[(Seated, &str)]) {
        log.shards_mut().for_each(|logs| logs.stamp(step));
        let mut arena = Arena::new(0);
        let records: Vec<_> = lines.iter().map(|(by, text)| says(&mut arena, *by, text)).collect();
        log.fold_records(records, &[arena]);
    }

    /// What `log` holds from `since` on, read in the step `step`: each entry's step and text, oldest first, the step
    /// to read on from, and how many had left.
    fn read_in(log: &mut Folded<Log>, step: u64, since: u64) -> (Vec<(u64, String)>, u64, u64) {
        log.shards_mut().for_each(|logs| logs.stamp(step));
        let read = recent(log, since, ActorName::Unnamed);
        let entries = read
            .entries
            .iter()
            .filter_map(|named| match named.recorded.entry() {
                Entry::Line { text, .. } => Some((named.recorded.step(), text.clone())),
                _ => None,
            })
            .collect();

        (entries, read.next, read.left)
    }

    /// Owned `(step, text)` pairs, to compare with a read.
    fn stepped(entries: &[(u64, &str)]) -> Vec<(u64, String)> {
        entries.iter().map(|(step, text)| (*step, String::from(*text))).collect()
    }

    #[test]
    fn actors_in_different_parts_are_read_merged_by_step() {
        // Catches a read that walks one part after another, so a later step of one part comes before an earlier step
        // of another, and one that orders a step's entries other than by part and then as each part folded them.
        // Seat 3 falls in part 1 and seat 4 in part 0.
        let odd = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let even = Seated { id: ActorId::new(2), seat: SeatNumber::new(4) };
        let mut log = Folded::<Log>::new(parts());
        fold_in(&mut log, 1, &[(odd, "odd 1"), (even, "even 1")]);
        fold_in(&mut log, 2, &[(odd, "odd 2a"), (odd, "odd 2b"), (even, "even 2")]);

        let (entries, ..) = read_in(&mut log, 3, 0);

        let expected = stepped(&[(1, "even 1"), (1, "odd 1"), (2, "even 2"), (2, "odd 2a"), (2, "odd 2b")]);
        assert_eq!(entries, expected);
    }

    #[test]
    fn a_read_from_its_next_step_sees_each_entry_once() {
        // Catches a read that answers entries before its step, and a `next` that repeats the last step read or skips
        // what was folded between two steps.
        let actor = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let mut log = Folded::<Log>::new(parts());
        fold_in(&mut log, 1, &[(actor, "one")]);
        fold_in(&mut log, 2, &[(actor, "two")]);

        let (first, next, _) = read_in(&mut log, 3, 2);
        // Folded after the read's step, and between it and the next.
        fold_in(&mut log, 3, &[(actor, "three"), (actor, "between")]);
        let (second, after, _) = read_in(&mut log, 4, next);

        assert_eq!((first, next), (stepped(&[(2, "two")]), 3));
        assert_eq!((second, after), (stepped(&[(3, "three"), (3, "between")]), 4));
    }

    #[test]
    fn a_full_ring_drops_its_oldest_and_counts_them_as_left() {
        // Catches a ring that grows without end, one that drops the newest, and entries that left uncounted, or
        // counted for a read from a step after theirs.
        let actor = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let mut log = Folded::<Log>::new(parts());
        log.shards_mut().for_each(|logs| logs.keep_recent_at_most(2));
        fold_in(&mut log, 1, &[(actor, "a")]);
        fold_in(&mut log, 2, &[(actor, "b"), (actor, "c")]);
        fold_in(&mut log, 3, &[(actor, "d")]);

        let (every, _, left) = read_in(&mut log, 4, 0);
        let (from_two, _, from_two_left) = read_in(&mut log, 4, 2);
        let (from_three, _, from_three_left) = read_in(&mut log, 4, 3);

        assert_eq!((every, left), (stepped(&[(2, "c"), (3, "d")]), 2));
        assert_eq!((from_two.len(), from_two_left), (2, 1));
        assert_eq!((from_three, from_three_left), (stepped(&[(3, "d")]), 0));
    }

    #[test]
    fn an_ended_actors_log_is_found_by_its_id_until_its_seat_logs_again() {
        // Catches a read by id that looks only at live actors, finds another actor's entries, or finds a log whose
        // seat another actor has since logged in.
        let ended = Seated { id: ActorId::new(1), seat: SeatNumber::new(3) };
        let other = Seated { id: ActorId::new(2), seat: SeatNumber::new(4) };
        let after = Seated { id: ActorId::new(3), seat: SeatNumber::new(3) };
        let mut arena = Arena::new(0);
        let lines = vec![logs(&mut arena, ended, 7), logs(&mut arena, other, 8)];
        let mut log = Folded::<Log>::new(parts());
        log.fold_records(lines, &[arena]);

        assert_eq!(texts(find(&log, ended.id)), ["7"]);
        assert_eq!(texts(find(&log, other.id)), ["8"]);

        let mut arena = Arena::new(0);
        let line = logs(&mut arena, after, 9);
        log.fold_records([line], &[arena]);

        assert_eq!(find(&log, ended.id).map(KeptLog::next), None);
        assert_eq!(texts(find(&log, after.id)), ["9"]);
    }
}
