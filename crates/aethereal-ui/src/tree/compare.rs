//! Comparing a tree with the one built before it (ADR-0056: A screen is an actor; *settled by the library's layout-cost
//! stage*): whether it is the same, differs only in what its widgets draw, or lays out otherwise, and which nodes lay
//! out otherwise, told to what keeps the last layouts so it lets go of what no longer holds.
//!
//! Two nodes at one key path lay out alike when their styles are the same, their widgets lay out alike
//! ([`Widget::same_layout`](crate::Widget::same_layout)), and their children stand at the same keys in the same order.
//! What a widget draws beside that, its settings layout does not read and what its markup drags and drops, is drawing
//! alone. A node that lays out otherwise, or draws otherwise, is told of, so what a screen keeps of its draws lets go of
//! a widget whose settings changed in any way (*settled by the library's draw-cost stage*).

use super::{Memo, Memos, Node, Segment, Stamps, Tree};
use core::mem;
use std::collections::BTreeMap;

/// How a tree differs from the last one built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difference {
    /// Not at all: the same tree, rebuilt.
    Same,
    /// Only in what some of its widgets draw: it lays out as the last one did.
    Drawing,
    /// In how some of it lays out: what a screen that has built nothing yet stands at.
    #[default]
    Layout,
}

impl Difference {
    /// It, with `other` beside it: the most of the two.
    const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Layout, _) | (_, Self::Layout) => Self::Layout,
            (Self::Drawing, _) | (_, Self::Drawing) => Self::Drawing,
            (Self::Same, Self::Same) => Self::Same,
        }
    }
}

/// What hears which nodes of a tree lay out otherwise than the last tree's at their keys, and which came and went: what
/// a screen keeps of its layouts and its texts, to let go of what no longer holds.
pub trait Relaid {
    /// The node at `path`, the root's segment first, lays out otherwise: its style, its widget's layout, or the keys of
    /// its children changed.
    fn changed(&mut self, path: &[Segment]);

    /// The node at `path` lays out as it did and draws otherwise: its widget's settings that layout does not read, or
    /// what its markup drags and drops, changed.
    fn redrawn(&mut self, path: &[Segment]);

    /// The node at `path` is new to the tree, with all within it: the last tree had none there.
    fn entered(&mut self, path: &[Segment]);

    /// The last tree's node at `path` is gone from the tree, with all within it.
    fn left(&mut self, path: &[Segment]);
}

/// A pair of nodes at one key path being merged, the last tree's and the new one's, and how deep they stand, the root
/// at one.
struct Pair<'old, 'new> {
    old: &'old mut Node,
    new: &'new mut Node,
    depth: usize,
}

impl Tree {
    /// Takes in this tree as the first a screen built, or the first since it moved: `memos` keeps what each composite
    /// marked `memo` in it was built from, and nothing else. It lays out as nothing did before it.
    pub(crate) fn first(&mut self, memos: &mut Memos, build: u64) -> Difference {
        memos.clear();
        let mut path = vec![self.root.segment];
        entered(&mut self.root, &mut path, memos, build);

        Difference::Layout
    }

    /// How `new` differs from this tree, the last one built, telling `relaid` each node of it that lays out or draws
    /// otherwise than this one's at its key, each new to it, and each of this one's gone from it; and `new` merged with
    /// it: each marker of a kept composite in `new` takes this tree's node at its path, moved out of this tree whole and
    /// neither walked nor compared, as its inputs stood; and `memos` keeps what each composite built in `new` was built
    /// from, and lets go of each that left. Walked with a stack of its own, so a tree's depth costs no frames.
    ///
    /// Each node of `new` is stamped with the build it last laid out and drew otherwise at ([`Stamps`]): `build`, this
    /// one, where it differs or is new, and its node's in this tree where it does not, so a node stands changed until a
    /// layout takes it in, however many builds come between.
    pub(crate) fn merge(
        &mut self,
        new: &mut Self,
        relaid: &mut impl Relaid,
        memos: &mut Memos,
        build: u64,
    ) -> Difference {
        let rooted = self.root.segment == new.root.segment;
        if !rooted {
            memos.clear();
            relaid.left(&[self.root.segment]);
            relaid.entered(&[new.root.segment]);
            let mut segments = vec![new.root.segment];
            entered(&mut new.root, &mut segments, memos, build);
            return Difference::Layout;
        }
        let mut difference = Difference::Same;
        let mut segments: Vec<Segment> = Vec::new();
        let mut stack = vec![Pair { old: &mut self.root, new: &mut new.root, depth: 1 }];
        while let Some(Pair { old, new, depth }) = stack.pop() {
            segments.truncate(depth - 1);
            segments.push(new.segment);
            let kept = matches!(new.memo, Memo::Kept);
            if kept {
                *new = mem::replace(old, Node::kept(new.segment));
                continue;
            }
            let (differs, keyed) = difference_of(old, new);
            new.stamps = match differs {
                Differs::Layout => {
                    relaid.changed(&segments);
                    difference = Difference::Layout;
                    Stamps::at(build)
                }
                Differs::Drawing => {
                    relaid.redrawn(&segments);
                    difference = difference.and(Difference::Drawing);
                    Stamps { laid: old.stamps.laid, drawn: build }
                }
                Differs::Not => old.stamps,
            };
            remembered(old, new, &segments, memos);
            let depth = depth + 1;
            if keyed {
                for (old, new) in old.children.iter_mut().zip(&mut new.children) {
                    stack.push(Pair { old, new, depth });
                }
                continue;
            }
            let pairs = paired(&mut old.children, &new.children);
            let paired_count = pairs.iter().filter(|pair| **pair != Paired::New).count();
            let (matched, gone) = old.children.split_at_mut(paired_count);
            let mut matched = matched.iter_mut();
            for (new, pair) in new.children.iter_mut().zip(pairs) {
                let old = match pair {
                    Paired::At(_) => matched.next(),
                    Paired::New => None,
                };
                if let Some(old) = old {
                    stack.push(Pair { old, new, depth });
                } else {
                    segments.push(new.segment);
                    relaid.entered(&segments);
                    entered(new, &mut segments, memos, build);
                    segments.pop();
                }
            }
            for old in &*gone {
                segments.push(old.segment);
                relaid.left(&segments);
                left(old, &mut segments, memos);
                segments.pop();
            }
        }

        difference
    }
}

/// How one node differs from the last tree's at its key path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Differs {
    Not,
    Drawing,
    Layout,
}

/// How `new` differs from `old`, the last tree's node at its key path: in its layout, when its style, its widget's
/// layout, or its children's keys changed; in its drawing alone, when its widget's other settings, or what it drags
/// or takes dropped, changed. Beside it, whether their children stand at the same keys in the same order.
fn difference_of(old: &Node, new: &Node) -> (Differs, bool) {
    let styled = old.style == new.style;
    let drawn = old.widget.same(&*new.widget);
    let laid = drawn || old.widget.same_layout(&*new.widget);
    let kept = same_keys(&old.children, &new.children);
    let lays_alike = styled && laid && kept;
    let dragged = old.drag == new.drag;
    let dropped = old.drop == new.drop;
    let draws_alike = drawn && dragged && dropped;
    let differs = match (lays_alike, draws_alike) {
        (false, _) => Differs::Layout,
        (true, false) => Differs::Drawing,
        (true, true) => Differs::Not,
    };

    (differs, kept)
}

/// Keeps what `new`, at `path`, was built from when it is a composite marked `memo` built at this view, and lets go of
/// what `old`, the last tree's node there, was built from when `new` is no such composite.
fn remembered(old: &Node, new: &mut Node, path: &[Segment], memos: &mut Memos) {
    let plain = matches!((&old.memo, &new.memo), (Memo::Plain, Memo::Plain));
    if plain {
        return;
    }
    match mem::replace(&mut new.memo, Memo::Plain) {
        Memo::Built(inputs) => {
            memos.keep(path, inputs);
            new.memo = Memo::Root;
        }
        Memo::Root => new.memo = Memo::Root,
        Memo::Plain | Memo::Kept => {
            let was = matches!(old.memo, Memo::Root);
            if was {
                memos.forget(path);
            }
        }
    }
}

/// Stamps `node`, at `path`, new to the tree with all within it, as entering at `build`, and keeps what each composite
/// marked `memo` in it was built from.
/// A marker there has no node to take, as only an item whose key a sibling before it has stands where no node stood,
/// and is left as it is, drawing nothing, with the item left out.
fn entered(node: &mut Node, path: &mut Vec<Segment>, memos: &mut Memos, build: u64) {
    let depth = path.len();
    let mut stack: Vec<(&mut Node, usize)> = vec![(node, depth)];
    while let Some((node, depth)) = stack.pop() {
        path.truncate(depth - 1);
        path.push(node.segment);
        node.stamps = Stamps::at(build);
        if let Memo::Built(inputs) = mem::replace(&mut node.memo, Memo::Plain) {
            memos.keep(path, inputs);
            node.memo = Memo::Root;
        }
        for child in &mut node.children {
            stack.push((child, depth + 1));
        }
    }
    path.truncate(depth);
}

/// Lets go of what each composite marked `memo` in `node`, at `path`, gone from the tree with all within it, was built
/// from.
fn left(node: &Node, path: &mut Vec<Segment>, memos: &mut Memos) {
    let depth = path.len();
    let mut stack: Vec<(&Node, usize)> = vec![(node, depth)];
    while let Some((node, depth)) = stack.pop() {
        path.truncate(depth - 1);
        path.push(node.segment);
        let root = matches!(node.memo, Memo::Root);
        if root {
            memos.forget(path);
        }
        for child in &node.children {
            stack.push((child, depth + 1));
        }
    }
    path.truncate(depth);
}

/// Whether `old` and `new` are children at the same keys, in the same order.
fn same_keys(old: &[Node], new: &[Node]) -> bool {
    let counted = old.len() == new.len();

    counted && old.iter().zip(new).all(|(old, new)| old.segment == new.segment)
}

/// Where a child of a new tree's node stands among the last tree's node's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paired {
    /// Beside the last tree's child at this place, the first there at its key.
    At(usize),
    /// Beside none: it is new.
    New,
}

/// Pairs the children of `old` with those of `new` by key, reordering `old` so that each child paired stands at the
/// place of its pair among those paired, in `new`'s order, and each paired with none after them: where each of `new`
/// stands. Walked in step while the keys stand in the same order, as a list grown at either end or with an item taken
/// out of it does, and found by key past the first that does not; `old` is reordered only when its pairs do not stand
/// in order already.
fn paired(old: &mut Vec<Node>, new: &[Node]) -> Vec<Paired> {
    let mut by: BTreeMap<Segment, usize> = BTreeMap::new();
    let mut indexed = false;
    let mut next = 0;
    let mut taken = vec![false; old.len()];
    let mut pairs = Vec::with_capacity(new.len());
    for child in new {
        let at_next = old.get(next).is_some_and(|old| old.segment == child.segment);
        let in_step = at_next && !taken[next];
        if in_step {
            taken[next] = true;
            pairs.push(Paired::At(next));
            next += 1;
            continue;
        }
        if !indexed {
            for (at, old) in old.iter().enumerate().rev() {
                by.insert(old.segment, at);
            }
            indexed = true;
        }
        let found = by.remove(&child.segment).filter(|at| !taken[*at]);
        let pair = found.map_or(Paired::New, |at| {
            taken[at] = true;
            Paired::At(at)
        });
        pairs.push(pair);
    }
    let mut ranks: Vec<usize> = (0..old.len()).map(|at| new.len() + at).collect();
    let mut rank = 0;
    for pair in &pairs {
        if let Paired::At(at) = pair {
            ranks[*at] = rank;
            rank += 1;
        }
    }
    let ordered = ranks.windows(2).all(|pair| pair[0] < pair[1]);
    if !ordered {
        let mut ranked: Vec<(usize, Node)> = ranks.into_iter().zip(old.drain(..)).collect();
        ranked.sort_unstable_by_key(|(rank, _)| *rank);
        old.extend(ranked.into_iter().map(|(_, node)| node));
    }

    pairs
}
