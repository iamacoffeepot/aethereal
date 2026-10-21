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

/// What came of asking for one thing as its maker: to write it, or to release it.
#[derive(Debug, PartialEq, Eq)]
pub enum Owned<T> {
    /// The asker made it: here it is, to write, or to drop once released.
    Mine(T),
    /// Another actor made it, and only that actor may release it.
    NotOwner,
    /// Nothing of this sort has the id: never made, or released already.
    Unknown,
}

impl<T> Owned<T> {
    /// What the asker made, as `f` makes it of what was asked for.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Owned<U> {
        match self {
            Self::Mine(value) => Owned::Mine(f(value)),
            Self::NotOwner => Owned::NotOwner,
            Self::Unknown => Owned::Unknown,
        }
    }
}

impl<T> Made<T> {
    /// Keeps `value` under `id`, made by `owner`.
    pub fn keep(&mut self, id: u64, owner: ActorId, value: T) {
        self.kept.insert(id, (owner, value));
    }

    /// Releases what is kept under `id`, when `by` made it.
    pub fn release(&mut self, id: u64, by: ActorId) -> Owned<T> {
        let Some((owner, _)) = self.kept.get(&id) else {
            return Owned::Unknown;
        };
        if *owner != by {
            return Owned::NotOwner;
        }

        self.kept.remove(&id).map_or(Owned::Unknown, |(_, value)| Owned::Mine(value))
    }

    /// Drops what is kept under `id`, whoever made it: what the renderer itself takes the place of.
    pub fn forget(&mut self, id: u64) {
        self.kept.remove(&id);
    }

    /// What is kept under `id`, to write, when `by` made it.
    pub fn owned(&mut self, id: u64, by: ActorId) -> Owned<&mut T> {
        match self.kept.get_mut(&id) {
            Some((owner, value)) if *owner == by => Owned::Mine(value),
            Some(_) => Owned::NotOwner,
            None => Owned::Unknown,
        }
    }

    /// Releases everything `owner` made, and answers it, each under its id.
    pub fn release_all(&mut self, owner: ActorId) -> Vec<(u64, T)> {
        self.kept.extract_if(|_, (made_by, _)| *made_by == owner).map(|(id, (_, value))| (id, value)).collect()
    }

    /// What is kept under `id`, whoever made it: what any actor handed the id may draw with.
    pub fn get(&self, id: u64) -> Option<&T> {
        self.kept.get(&id).map(|(_, value)| value)
    }

    /// The actor that made what is kept under `id`: none when nothing is.
    pub fn maker(&self, id: u64) -> Option<ActorId> {
        self.kept.get(&id).map(|(maker, _)| *maker)
    }

    /// What is kept under `id`, whoever made it, to change as its keeper.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut T> {
        self.kept.get_mut(&id).map(|(_, value)| value)
    }

    /// Everything kept, each under its id, in no order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.kept.iter().map(|(id, (_, value))| (*id, value))
    }

    /// Everything kept, in no order.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.kept.values().map(|(_, value)| value)
    }

    /// Everything kept, in no order, to change.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.kept.values_mut().map(|(_, value)| value)
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
