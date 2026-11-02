//! The launch copies the store has handed out, and whose each is (ADR-0040).

use aethereal_actor::ErasedActorRef;
use aethereal_data::Bound;
use std::mem;

/// One launch copy, and the actor it was made for.
#[derive(Bound)]
struct Handed {
    asker: ErasedActorRef,
    copy: String,
}

/// The launch copies the store has handed out and not yet removed, and the count the next takes. Never written: the
/// store removes every launch copy as it begins, so its count starts again at nought.
#[derive(Bound, Default)]
pub struct Launches {
    /// Each launch copy each live actor asked for.
    handed: Vec<Handed>,
    /// The count the next launch copy takes.
    next: u64,
}

impl Launches {
    /// Names a launch copy of the binary `name` for `asker`, `<name>-<n>` with `n` the next count, and keeps it as
    /// `asker`'s until it ends.
    pub fn hand(&mut self, asker: ErasedActorRef, name: &str) -> String {
        let copy = format!("{name}-{}", self.next);
        self.next += 1;
        self.handed.push(Handed { asker, copy: copy.clone() });

        copy
    }

    /// Lets go of every launch copy of `asker`, which has ended, and answers their names, to remove.
    pub fn ended(&mut self, asker: ErasedActorRef) -> Vec<String> {
        let (gone, kept): (Vec<Handed>, Vec<Handed>) =
            mem::take(&mut self.handed).into_iter().partition(|handed| handed.asker == asker);
        self.handed = kept;

        gone.into_iter().map(|handed| handed.copy).collect()
    }
}
