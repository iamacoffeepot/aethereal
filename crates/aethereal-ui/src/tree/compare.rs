//! Comparing a tree with the one built before it (ADR-0056: A screen is an actor; *settled by the library's layout-cost
//! stage*): whether it is the same, differs only in what its widgets draw, or lays out otherwise, and which nodes lay
//! out otherwise, told to what keeps the last layouts so it lets go of what no longer holds.
//!
//! Two nodes at one key path lay out alike when their styles are the same, their widgets lay out alike
//! ([`Widget::same_layout`](crate::Widget::same_layout)), and their children stand at the same keys in the same order.
//! What a widget draws beside that, its settings layout does not read and what its markup drags and drops, is drawing
//! alone. A node that lays out otherwise, or draws otherwise, is told of, so what a screen keeps of its draws lets go of
//! a widget whose settings changed in any way (*settled by the library's draw-cost stage*).

use super::{Node, Segment, Tree};
use std::collections::BTreeMap;

/// How a tree differs from the last one built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difference {
    /// Not at all: the same tree, rebuilt.
    Same,
    /// Only in what some of its widgets draw: it lays out as the last one did.
    Drawing,
    /// In how some of it lays out.
    Layout,
}

impl Difference {
    /// It, with some widget's drawing differing beside what it says.
    const fn with_drawing(self) -> Self {
        match self {
            Self::Same | Self::Drawing => Self::Drawing,
            Self::Layout => Self::Layout,
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

/// Two hearers hear alike.
impl<One: Relaid, Other: Relaid> Relaid for (&mut One, &mut Other) {
    fn changed(&mut self, path: &[Segment]) {
        self.0.changed(path);
        self.1.changed(path);
    }

    fn redrawn(&mut self, path: &[Segment]) {
        self.0.redrawn(path);
        self.1.redrawn(path);
    }

    fn entered(&mut self, path: &[Segment]) {
        self.0.entered(path);
        self.1.entered(path);
    }

    fn left(&mut self, path: &[Segment]) {
        self.0.left(path);
        self.1.left(path);
    }
}

/// Three hearers hear alike.
impl<One: Relaid, Other: Relaid, Third: Relaid> Relaid for (&mut One, &mut Other, &mut Third) {
    fn changed(&mut self, path: &[Segment]) {
        self.0.changed(path);
        self.1.changed(path);
        self.2.changed(path);
    }

    fn redrawn(&mut self, path: &[Segment]) {
        self.0.redrawn(path);
        self.1.redrawn(path);
        self.2.redrawn(path);
    }

    fn entered(&mut self, path: &[Segment]) {
        self.0.entered(path);
        self.1.entered(path);
        self.2.entered(path);
    }

    fn left(&mut self, path: &[Segment]) {
        self.0.left(path);
        self.1.left(path);
        self.2.left(path);
    }
}

/// A pair of nodes at one key path being compared, the last tree's and the new one's, and how deep they stand, the root
/// at one.
struct Pair<'old, 'new> {
    old: &'old Node,
    new: &'new Node,
    depth: usize,
}

impl Tree {
    /// How `new` differs from this tree, the last one built, telling `relaid` each node of it that lays out otherwise
    /// than this one's at its key, each new to it, and each of this one's gone from it: walked with a stack of its own,
    /// so a tree's depth costs no frames.
    ///
    /// Each pair of nodes is compared once, in one walk, so a tree at rest costs what comparing it by value would, and
    /// one that changed costs no more (*settled by the library's draw-cost stage*).
    pub(crate) fn compare(&self, new: &Self, relaid: &mut impl Relaid) -> Difference {
        let rooted = self.root.segment == new.root.segment;
        if !rooted {
            relaid.left(&[self.root.segment]);
            relaid.entered(&[new.root.segment]);
            return Difference::Layout;
        }
        let mut difference = Difference::Same;
        let mut segments: Vec<Segment> = Vec::new();
        let mut stack = vec![Pair { old: &self.root, new: &new.root, depth: 1 }];
        while let Some(Pair { old, new, depth }) = stack.pop() {
            segments.truncate(depth - 1);
            segments.push(new.segment);
            let styled = old.style == new.style;
            let drawn = old.widget.same(&*new.widget);
            let laid = drawn || old.widget.same_layout(&*new.widget);
            let kept = same_keys(&old.children, &new.children);
            let lays_alike = styled && laid && kept;
            let dragged = old.drag == new.drag;
            let dropped = old.drop == new.drop;
            let draws_alike = drawn && dragged && dropped;
            difference = match (lays_alike, draws_alike) {
                (false, _) => {
                    relaid.changed(&segments);
                    Difference::Layout
                }
                (true, false) => {
                    relaid.redrawn(&segments);
                    difference.with_drawing()
                }
                (true, true) => difference,
            };
            let depth = depth + 1;
            if kept {
                for (old, new) in old.children.iter().zip(&new.children) {
                    stack.push(Pair { old, new, depth });
                }
                continue;
            }
            let mut olds: BTreeMap<Segment, &Node> = BTreeMap::new();
            for child in &old.children {
                olds.entry(child.segment).or_insert(child);
            }
            for child in &new.children {
                let Some(old) = olds.remove(&child.segment) else {
                    segments.push(child.segment);
                    relaid.entered(&segments);
                    segments.pop();
                    continue;
                };
                stack.push(Pair { old, new: child, depth });
            }
            for gone in olds.into_keys() {
                segments.push(gone);
                relaid.left(&segments);
                segments.pop();
            }
        }

        difference
    }
}

/// Whether `old` and `new` are children at the same keys, in the same order.
fn same_keys(old: &[Node], new: &[Node]) -> bool {
    let counted = old.len() == new.len();

    counted && old.iter().zip(new).all(|(old, new)| old.segment == new.segment)
}
