//! The view of what each actor logged most recently (ADR-0009, ADR-0011).

use crate::{BySeat, Context, Folded, Part, View};
use aethereal_actor::{
    Arena, Effect, Entry, Fault, KeptLog, Level, Line, Payload, SeatNumber, Seated, Unreadable, What,
};
use aethereal_data::ActorId;
use std::num::NonZeroUsize;

/// How many entries the view keeps for one actor by default, as `EngineSettings::most_logged` sets it (ADR-0044). An
/// older one leaves as a newer one arrives.
pub const MOST_LOGGED: usize = 32;

/// The view of what each actor logged most recently, keyed by the actor's seat (ADR-0011).
///
/// A key has one owner: an entry is by the actor it is kept under, so one
/// actor's entries arrive in the order it made them, and are numbered in that
/// order (ADR-0044). An actor's entries stay once it has ended, since that is
/// when why it ended is read, until another actor takes its seat and logs: a
/// log worth keeping longer is copied out before then.
pub struct Log;

/// One part of the log: the entries of each actor whose seat falls in it, and how many each keeps.
pub struct Logs {
    seats: BySeat<KeptLog>,
    /// The most entries an actor keeps: fixed as the engine is set up, before any fold.
    most: usize,
}

impl Part for Logs {
    fn empty(parts: NonZeroUsize) -> Self {
        Self { seats: BySeat::empty(parts), most: MOST_LOGGED }
    }
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
}

/// What the actor `id` has logged, live or ended, found in the seat it last logged in while no other actor has logged
/// there since: read by id, for a log read by path, as the view keeps it under seats (ADR-0044).
pub fn find(log: &Folded<Log>, id: ActorId) -> Option<&KeptLog> {
    log.shards().enumerate().find_map(|(part, logs)| {
        let mut seated = logs.seats.seated(part);

        seated.find(|(seated, _)| seated.id == id).map(|(_, kept)| kept)
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

    /// A line's text is copied out of its arena into the actor's entries, into the text of the entry that leaves as
    /// it arrives, so a log kept full allocates nothing.
    fn fold(shard: &mut Logs, key: u64, (id, logged): (ActorId, Logged), written: &[Arena]) {
        let most = shard.most;
        let kept = shard.seats.entry(Seated { id, seat: SeatNumber::new(key) }, KeptLog::default);
        kept.keep(most, |left| entry(logged, left, written));
    }
}

/// The entry `logged` comes to, a line's text copied out of its arena into the text of `left`, the entry that leaves
/// as it arrives, so a log kept full allocates nothing.
fn entry(logged: Logged, left: Option<Entry>, written: &[Arena]) -> Entry {
    match logged {
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
