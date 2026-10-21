//! What the renderer has made, by id, with the actor that made each: the only actor that may release it, and whose
//! ending releases it (ADR-0021).

#[cfg(test)]
mod tests;

use aethereal_data::ActorId;
use std::collections::{HashMap, HashSet};

/// The ids the renderer gives what it makes, from one count it never takes back, so an id once released names
/// nothing again.
#[derive(Debug, Default)]
pub struct Ids {
    next: u64,
}

impl Ids {
    /// The next id.
    pub const fn next(&mut self) -> u64 {
        self.next += 1;

        self.next
    }
}

/// What the renderer has made of one sort, by id, each beside the actor that made it.
#[derive(Debug)]
pub struct Made<T> {
    kept: HashMap<u64, (ActorId, T)>,
}

impl<T> Default for Made<T> {
    fn default() -> Self {
        Self { kept: HashMap::new() }
    }
}

/// What came of asking to release one thing.
#[derive(Debug, PartialEq, Eq)]
pub enum Released<T> {
    /// It was released: here it is, to drop.
    Freed(T),
    /// Another actor made it, and only that actor may release it.
    NotOwner,
    /// Nothing of this sort has the id: never made, or released already.
    Unknown,
}

impl<T> Released<T> {
    /// What was freed, as `f` makes it of what was released.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Released<U> {
        match self {
            Self::Freed(value) => Released::Freed(f(value)),
            Self::NotOwner => Released::NotOwner,
            Self::Unknown => Released::Unknown,
        }
    }
}

impl<T> Made<T> {
    /// Keeps `value` under `id`, made by `owner`.
    pub fn keep(&mut self, id: u64, owner: ActorId, value: T) {
        self.kept.insert(id, (owner, value));
    }

    /// Releases what is kept under `id`, when `by` made it.
    pub fn release(&mut self, id: u64, by: ActorId) -> Released<T> {
        let Some((owner, _)) = self.kept.get(&id) else {
            return Released::Unknown;
        };
        if *owner != by {
            return Released::NotOwner;
        }

        self.kept.remove(&id).map_or(Released::Unknown, |(_, value)| Released::Freed(value))
    }

    /// Releases everything `owner` made, and answers how many there were.
    pub fn release_all(&mut self, owner: ActorId) -> usize {
        let before = self.kept.len();
        self.kept.retain(|_, (made_by, _)| *made_by != owner);

        before - self.kept.len()
    }
}

/// The actors the renderer watches, so what each made is released when it ends: each actor that has made something,
/// from the first thing it made until it ends.
#[derive(Debug, Default)]
pub struct Makers {
    watched: HashSet<ActorId>,
}

impl Makers {
    /// Notes that `maker` made something, and answers whether it is the first: the renderer watches it from then.
    pub fn made(&mut self, maker: ActorId) -> bool {
        self.watched.insert(maker)
    }

    /// Notes that `maker` ended, and answers whether it was watched: what it made is released.
    pub fn ended(&mut self, maker: ActorId) -> bool {
        self.watched.remove(&maker)
    }
}
