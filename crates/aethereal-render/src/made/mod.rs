//! What the renderer has made, by id, with the actor that made each: the only actor that may release it, and whose
//! ending releases it (ADR-0021).

#[cfg(test)]
mod tests;

use crate::stats::GpuMemory;
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

    /// What is kept under `id`, to write, when `by` made it.
    pub fn owned(&mut self, id: u64, by: ActorId) -> Owned<&mut T> {
        match self.kept.get_mut(&id) {
            Some((owner, value)) if *owner == by => Owned::Mine(value),
            Some(_) => Owned::NotOwner,
            None => Owned::Unknown,
        }
    }

    /// Drops what is kept under `id`, whoever made it: what the renderer itself is done with.
    pub fn forget(&mut self, id: u64) {
        self.kept.remove(&id);
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

    /// How many are kept, as a count of things alive, far below what a `u32` counts (ADR-0053).
    pub fn alive(&self) -> u32 {
        u32::try_from(self.kept.len()).unwrap_or(u32::MAX)
    }
}

/// What a thing the renderer made holds of the GPU's memory, by kind (ADR-0053).
pub trait Weighs {
    /// The bytes it holds, each under its kind.
    fn memory(&self) -> GpuMemory;
}

/// Changes `value` as `change` does, keeping `total`, which counts `value` in, counting it as it is after.
pub fn reweighed<T: Weighs, R>(total: &mut GpuMemory, value: &mut T, change: impl FnOnce(&mut T) -> R) -> R {
    total.remove(value.memory());
    let changed = change(value);
    total.add(value.memory());

    changed
}

/// What the renderer has made of one sort that holds GPU memory, as [`Made`] keeps it, with the memory all of it
/// holds, kept as it is made, changed, and released, so it is read without a walk (ADR-0053). What is kept is changed
/// only through a change that weighs it again.
#[derive(Debug)]
pub struct Weighed<T> {
    made: Made<T>,
    memory: GpuMemory,
}

impl<T> Default for Weighed<T> {
    fn default() -> Self {
        Self { made: Made::default(), memory: GpuMemory::default() }
    }
}

impl<T: Weighs> Weighed<T> {
    /// Keeps `value` under `id`, made by `owner`, as [`Made::keep`] does.
    pub fn keep(&mut self, id: u64, owner: ActorId, value: T) {
        self.memory.add(value.memory());
        let replaced = self.made.kept.insert(id, (owner, value));
        if let Some((_, replaced)) = replaced {
            self.memory.remove(replaced.memory());
        }
    }

    /// Releases what is kept under `id`, when `by` made it, as [`Made::release`] does.
    pub fn release(&mut self, id: u64, by: ActorId) -> Owned<T> {
        let released = self.made.release(id, by);
        if let Owned::Mine(value) = &released {
            self.memory.remove(value.memory());
        }

        released
    }

    /// Drops what is kept under `id`, whoever made it: what the renderer itself takes the place of.
    pub fn forget(&mut self, id: u64) {
        if let Some((_, value)) = self.made.kept.remove(&id) {
            self.memory.remove(value.memory());
        }
    }

    /// Releases everything `owner` made, and answers it, as [`Made::release_all`] does.
    pub fn release_all(&mut self, owner: ActorId) -> Vec<(u64, T)> {
        let released = self.made.release_all(owner);
        for (_, value) in &released {
            self.memory.remove(value.memory());
        }

        released
    }

    /// Changes what is kept under `id` as `change` does, when `by` made it, as [`Made::owned`] lets it.
    pub fn owned<R>(&mut self, id: u64, by: ActorId, change: impl FnOnce(&mut T) -> R) -> Owned<R> {
        let memory = &mut self.memory;
        self.made.owned(id, by).map(|value| reweighed(memory, value, change))
    }

    /// Changes what is kept under `id`, whoever made it, as `change` does, as its keeper: none when nothing is.
    pub fn change<R>(&mut self, id: u64, change: impl FnOnce(&mut T) -> R) -> Option<R> {
        let memory = &mut self.memory;
        self.made.get_mut(id).map(|value| reweighed(memory, value, change))
    }

    /// Changes everything kept as `change` does, in no order.
    pub fn change_each(&mut self, mut change: impl FnMut(&mut T)) {
        for value in self.made.values_mut() {
            reweighed(&mut self.memory, value, &mut change);
        }
    }
}

impl<T> Weighed<T> {
    /// What is kept under `id`, whoever made it, as [`Made::get`] answers.
    pub fn get(&self, id: u64) -> Option<&T> {
        self.made.get(id)
    }

    /// The actor that made what is kept under `id`, as [`Made::maker`] answers.
    pub fn maker(&self, id: u64) -> Option<ActorId> {
        self.made.maker(id)
    }

    /// Everything kept, each under its id, in no order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.made.iter()
    }

    /// Everything kept, in no order.
    #[cfg(test)]
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.made.values()
    }

    /// How many are kept, as [`Made::alive`] counts them.
    pub fn alive(&self) -> u32 {
        self.made.alive()
    }

    /// The GPU memory all that is kept holds.
    pub const fn memory(&self) -> GpuMemory {
        self.memory
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
