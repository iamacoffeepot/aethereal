//! A view: state many actors read, folded from the effects of finished turns (ADR-0011).

use crate::Context;
use aethereal_actor::Effect;
use std::mem;
use std::num::NonZeroUsize;

/// A view: what it keeps of an effect, and how one such record changes its state (ADR-0011).
///
/// Its author writes these and nothing about threads. The engine keeps the
/// view in a [`Folded`], which does the rest the same way for every view.
///
/// A key has one owner, whose records reach [`Self::fold`] in the order it
/// asked for them. Or many actors write a key, and then the fold gives the
/// same state in whatever order their records arrive.
///
/// Each effect is folded with the [`Context`] it was emitted in: the item its
/// turn was reacting to, which every effect of that turn shares. A view reads
/// nothing else, and no other view: each view folds by itself, in whatever
/// order and wherever the engine folds it.
pub trait View {
    /// One part of the view's state.
    type Shard;
    /// What the view keeps of an effect.
    type Record;

    /// The keys and records of an effect, one for each key it changes: none for an effect this view does not fold.
    ///
    /// `context` is the context the effect was emitted in.
    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Self::Record)> + 'a;

    /// Folds one record into the part its key falls in.
    fn fold(shard: &mut Self::Shard, key: u64, record: Self::Record);

    /// Folds one writer's records for a part, in the order the writer routed them, and leaves the list empty, with
    /// its room. One by one, unless the part keeps its records as they come and takes the list whole.
    fn fold_all(shard: &mut Self::Shard, records: &mut Vec<(u64, Self::Record)>) {
        for (key, record) in records.drain(..) {
            Self::fold(shard, key, record);
        }
    }
}

/// One part of a view's state, made empty for its place among `parts` parts.
///
/// A part that keeps nothing by where it lies among the parts is its
/// `Default`. One kept by seat, a [`crate::BySeat`], knows how many parts
/// there are, since it holds every `parts`th seat.
pub trait Part {
    /// The part before any effect, one of `parts`.
    fn empty(parts: NonZeroUsize) -> Self;
}

impl<T: Default> Part for T {
    fn empty(_: NonZeroUsize) -> Self {
        T::default()
    }
}

/// A view that can take an effect by value, so what it keeps of the effect is moved in and not copied.
///
/// Only the last view to fold a turn's effects can take them, since nothing
/// reads an effect after it is taken. [`View::takes`] gives the same records
/// from a borrowed effect.
pub trait Consumes: View {
    /// The keys and records of `effect`, taken by value, as [`View::takes`] gives them.
    fn takes_owned(effect: Effect, context: &Context) -> impl Iterator<Item = (u64, Self::Record)> + '_;
}

/// A view as the engine keeps it: its state in parts, each holding the keys that fall in it (ADR-0011).
///
/// While a step's turns run they read it, and nothing writes it. When they
/// have finished, the step's effects are folded in. Folding takes the view
/// exclusively, so the two never happen together and nothing here is locked.
pub struct Folded<V: View> {
    /// The parts. A key always falls in the same one.
    shards: Vec<V::Shard>,
}

impl<V: View> Folded<V> {
    /// The view before any effect, its keys spread over `parts` parts.
    #[must_use]
    pub fn new(parts: NonZeroUsize) -> Self
    where
        V::Shard: Part,
    {
        let parts = rounded(parts);

        Self { shards: (0..parts.get()).map(|_| V::Shard::empty(parts)).collect() }
    }

    /// Folds in the effects of one turn, emitted in `context`, in the order given.
    pub fn fold(&mut self, effects: &[Effect], context: &Context) {
        for (key, record) in effects.iter().flat_map(|effect| V::takes(effect, context)) {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// Folds in the effects of one turn, emitted in `context`, taking each by value.
    ///
    /// The caller folds every other view that reads these effects first.
    pub fn fold_owned(&mut self, effects: impl IntoIterator<Item = Effect>, context: &Context)
    where
        V: Consumes,
    {
        for (key, record) in effects.into_iter().flat_map(|effect| V::takes_owned(effect, context)) {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// An empty row of buckets for one writer, a bucket for each part of this view.
    #[must_use]
    pub fn routed(&self) -> Routed<V> {
        Routed { buckets: self.shards.iter().map(|_| Vec::new()).collect() }
    }

    /// Folds in every writer's records of a step, part by part: each part takes its column, the bucket each row of
    /// `rows` holds for it, one row after another. The buckets are emptied and keep their room.
    ///
    /// The records of a key that has one owner come from that owner's turns, which one writer ran, in order, so they
    /// reach [`View::fold`] in the order the owner asked for them.
    pub fn fold_routed<'r>(&mut self, rows: impl IntoIterator<Item = &'r mut Routed<V>>)
    where
        V: 'r,
    {
        let mut rows: Vec<&mut Routed<V>> = rows.into_iter().collect();
        for (part, shard) in self.shards.iter_mut().enumerate() {
            for row in &mut rows {
                V::fold_all(shard, &mut row.buckets[part]);
            }
        }
    }

    /// Folds every writer's records for the part numbered `part` where it lies, one row after another: what
    /// [`Column::fold`] does with the part taken out. The buckets are emptied and keep their room.
    pub fn fold_part<'r>(&mut self, part: usize, rows: impl IntoIterator<Item = &'r mut Routed<V>>)
    where
        V: 'r,
    {
        for row in rows {
            V::fold_all(&mut self.shards[part], &mut row.buckets[part]);
        }
    }

    /// The part numbered `part`, to change outside a fold, as [`Self::shard_mut`] gives one.
    pub fn at_mut(&mut self, part: usize) -> &mut V::Shard {
        &mut self.shards[part]
    }

    /// The part `key` falls in, as of the last fold.
    #[must_use]
    pub fn shard(&self, key: u64) -> &V::Shard {
        &self.shards[self.part(key)]
    }

    /// The part `key` falls in, to change outside a fold: for the engine, which takes from a view what it hands
    /// out, such as the items an actor is to react to, between steps.
    pub fn shard_mut(&mut self, key: u64) -> &mut V::Shard {
        let part = self.part(key);

        &mut self.shards[part]
    }

    /// The part numbered `part`, as of the last fold.
    #[must_use]
    pub fn at(&self, part: usize) -> &V::Shard {
        &self.shards[part]
    }

    /// Takes the part numbered `part` out, leaving an empty one in its place: for a worker to own while a step runs.
    /// [`Self::put`] puts it back.
    pub fn take(&mut self, part: usize) -> V::Shard
    where
        V::Shard: Part,
    {
        let parts = NonZeroUsize::new(self.shards.len()).unwrap_or(NonZeroUsize::MIN);

        mem::replace(&mut self.shards[part], V::Shard::empty(parts))
    }

    /// Puts back the part numbered `part`, taken out by [`Self::take`].
    pub fn put(&mut self, part: usize, shard: V::Shard) {
        self.shards[part] = shard;
    }

    /// Every part, as of the last fold.
    pub fn shards(&self) -> impl Iterator<Item = &V::Shard> {
        self.shards.iter()
    }

    /// Every part, to change outside a fold, as [`Self::shard_mut`] gives one.
    pub fn shards_mut(&mut self) -> impl Iterator<Item = &mut V::Shard> {
        self.shards.iter_mut()
    }

    /// Which part `key` falls in.
    fn part(&self, key: u64) -> usize {
        part(key, self.shards.len())
    }
}

/// One part of a view taken out with every writer's bucket for it, for a worker to fold by itself (ADR-0011).
///
/// A part depends only on the records of its own keys, so the parts of a
/// step's fold are folded apart, each by one worker, and put back.
pub struct Column<V: View> {
    shard: V::Shard,
    /// Each writer's bucket for this part, in the writers' order: emptied as it is folded, and handed back to its
    /// row for its room.
    buckets: Vec<Vec<(u64, V::Record)>>,
}

impl<V: View> Column<V> {
    /// Folds every writer's bucket into the part, one writer after another.
    pub fn fold(&mut self) {
        for bucket in &mut self.buckets {
            V::fold_all(&mut self.shard, bucket);
        }
    }

    /// The part, as folded so far.
    #[must_use]
    pub const fn shard(&self) -> &V::Shard {
        &self.shard
    }

    /// The part, to change once it is folded: to settle it.
    pub const fn shard_mut(&mut self) -> &mut V::Shard {
        &mut self.shard
    }

    /// How many records wait to be folded.
    #[must_use]
    pub fn records(&self) -> usize {
        self.buckets.iter().map(Vec::len).sum()
    }
}

impl<V: View> Column<V> {
    /// An empty column, for a view kept in `parts` parts: to be filled and emptied again step after step.
    #[must_use]
    pub fn empty(parts: NonZeroUsize) -> Self
    where
        V::Shard: Part,
    {
        Self { shard: V::Shard::empty(parts), buckets: Vec::new() }
    }
}

impl<V: View> Folded<V> {
    /// Fills `column` with the part numbered `part`, taken out, and each row's bucket for it, to fold apart from
    /// the rest.
    pub fn fill<'r>(&mut self, part: usize, rows: impl IntoIterator<Item = &'r mut Routed<V>>, column: &mut Column<V>)
    where
        V::Shard: Part,
        V: 'r,
    {
        column.shard = self.take(part);
        column.buckets.extend(rows.into_iter().map(|row| mem::take(&mut row.buckets[part])));
    }

    /// Empties `column`, filled from the part numbered `part`: puts the part back, and hands each row of `rows` its
    /// emptied bucket.
    pub fn empty<'r>(&mut self, part: usize, column: &mut Column<V>, rows: impl IntoIterator<Item = &'r mut Routed<V>>)
    where
        V::Shard: Part,
        V: 'r,
    {
        let parts = NonZeroUsize::new(self.shards.len()).unwrap_or(NonZeroUsize::MIN);
        self.put(part, mem::replace(&mut column.shard, V::Shard::empty(parts)));
        for (row, bucket) in rows.into_iter().zip(column.buckets.drain(..)) {
            row.buckets[part] = bucket;
        }
    }

    /// Folds records made outside a step's turns, each into the part its key falls in.
    pub fn fold_records(&mut self, records: impl IntoIterator<Item = (u64, V::Record)>) {
        for (key, record) in records {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }
}

/// One writer's records of a view in a step, each in the bucket of the part its key falls in: the writer's row of
/// buckets (ADR-0011).
///
/// A worker routes its turns' effects into its row as each turn finishes,
/// and [`Folded::fold_routed`] folds each part's column of the rows.
pub struct Routed<V: View> {
    /// A bucket for each part of the view.
    buckets: Vec<Vec<(u64, V::Record)>>,
}

impl<V: View> Default for Routed<V> {
    /// A row with no bucket: what a worker's lists hold while they are lent out, never routed into.
    fn default() -> Self {
        Self { buckets: Vec::new() }
    }
}

impl<V: View> Routed<V> {
    /// How many records this row holds for the part numbered `part`.
    #[must_use]
    pub fn records(&self, part: usize) -> usize {
        self.buckets[part].len()
    }

    /// Puts one record under `key` in the bucket of the part it falls in.
    pub fn put(&mut self, key: u64, record: V::Record) {
        let part = part(key, self.buckets.len());
        self.buckets[part].push((key, record));
    }

    /// Routes one effect, emitted in `context`.
    pub fn route(&mut self, effect: &Effect, context: &Context) {
        for (key, record) in V::takes(effect, context) {
            let part = part(key, self.buckets.len());
            self.buckets[part].push((key, record));
        }
    }

    /// Routes one effect, emitted in `context`, taking it by value.
    ///
    /// The caller routes every other view that reads this effect first.
    pub fn route_one(&mut self, effect: Effect, context: &Context)
    where
        V: Consumes,
    {
        for (key, record) in V::takes_owned(effect, context) {
            let part = part(key, self.buckets.len());
            self.buckets[part].push((key, record));
        }
    }
}

/// Which of `parts` parts `key` falls in, for a count of parts [`rounded`] to a power of two.
fn part(key: u64, parts: usize) -> usize {
    #[allow(clippy::cast_possible_truncation, reason = "the part is less than the count of parts, a usize")]
    let part = (key & (parts as u64).wrapping_sub(1)) as usize;

    part
}

/// `parts` rounded up to a power of two: how many parts a view is kept in.
///
/// The part a key falls in is then a mask of the key, and its place in that part a shift, where a division would
/// cost tens of cycles on every lookup of every actor.
///
/// # Panics
///
/// Panics for more parts than half of what a `usize` counts, which no engine has.
#[must_use]
pub fn rounded(parts: NonZeroUsize) -> NonZeroUsize {
    let Some(rounded) = parts.checked_next_power_of_two() else {
        panic!("no engine keeps its views in more than half of usize::MAX parts");
    };

    rounded
}
