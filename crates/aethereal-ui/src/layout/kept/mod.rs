//! What a screen keeps of its layouts from one to the next (ADR-0056: Layout is Taffy; *settled by the library's
//! layout-cost stage*): the last boxes, handed back while its tree lays out as it did, and what Taffy keeps of each
//! node, so a layout lays out again only the nodes whose layout changed and those that hold them, as Taffy's own tree
//! does when a node is marked dirty.

#[cfg(test)]
mod tests;

use super::{Laidout, Laying, Tree, boxed, in_flow};
use crate::fonts::{Asker, Measuring};
use crate::style::{Parent, flow};
use crate::tree::{Flat, KeyPath, Relaid, Segment};
use aethereal_math::Vec2;
use core::mem;
use std::collections::BTreeMap;
use taffy::{AvailableSpace, Cache, Dimension, Layout, NodeId, Position, Size, compute_root_layout};

/// What a screen keeps of its layouts from one to the next (*settled by the library's layout-cost stage*): what Taffy
/// kept of each node, by its key path, and the boxes of the last layout, with whether they hold.
///
/// A layout whose tree lays out as the last one did, at the same size, scale, and open popups, is the last one: its
/// boxes are handed back, and Taffy does not run. Otherwise Taffy runs, and a node whose layout is kept, whose own
/// settings and whose children's laid out as they did, answers from what it kept, so only what changed and what holds
/// it is laid out again. What tells it what changed is the comparison of each tree with the last ([`Relaid`]): a node
/// whose style, widget's layout, or children changed lets go of what it kept, as does every node holding it, and a node
/// new to the tree, or one whose cache is empty, is laid out again with all that holds it. Every node kept is let go at
/// another UI scale, which every length and measure reads, and after a layout that met a font whose metrics had not
/// come. What a node's style as Taffy reads it takes beside its own, its room and what its parent and its place make of
/// it, follows from what the comparison tells and the size, scale, and popups the boxes hold at.
#[derive(Debug, Default)]
pub struct Layouts {
    /// What Taffy kept of each node, by its key path, as the last layout left it.
    nodes: BTreeMap<KeyPath, Kept>,
    /// The UI scale the nodes kept were laid out at.
    scale: f32,
    /// The boxes the last layout gave.
    laidout: Laidout,
    holds: Holds,
}

/// Whether the boxes a screen's last layout gave hold for the tree it lays out next.
#[derive(Debug, Default)]
enum Holds {
    /// They do not: nothing was laid out, the tree laid out otherwise since, or a font's metrics had not come.
    #[default]
    Not,
    /// They do while the tree lays out as it did, at what this says.
    At(LaidAt),
}

/// What a layout was laid out at beside its tree: the screen's size and UI scale, and the places of the open popups'
/// content.
#[derive(Debug)]
struct LaidAt {
    size: Vec2,
    scale: f32,
    tops: Vec<usize>,
}

impl LaidAt {
    /// Whether it is `size`, `scale`, and `tops`.
    fn is(&self, size: Vec2, scale: f32, tops: &[usize]) -> bool {
        let sized = self.size == size;
        let scaled = self.scale.to_bits() == scale.to_bits();
        let topped = self.tops == tops;

        sized && scaled && topped
    }
}

/// What Taffy keeps of one node from one layout to the next: its place in tree order when it was kept, which Taffy
/// names it by; Taffy's cache of what it laid out; where its parent laid it; and the boxes out of flow it holds, by
/// their places.
#[derive(Debug)]
pub(super) struct Kept {
    at: usize,
    pub(super) cache: Cache,
    pub(super) layout: Layout,
    pub(super) hoisted: Vec<NodeId>,
}

impl Kept {
    /// A node at `at`, laid out nowhere yet.
    const fn new(at: usize) -> Self {
        Self { at, cache: Cache::new(), layout: Layout::with_order(0), hoisted: Vec::new() }
    }

    /// It, at `at` in tree order: what it hoisted moves with it, as its subtree stands unchanged.
    fn moved(mut self, at: usize) -> Self {
        let moved = at != self.at;
        if moved {
            for hoisted in &mut self.hoisted {
                *hoisted = NodeId::from(usize::from(*hoisted) + at - self.at);
            }
            self.at = at;
        }

        self
    }
}

impl Layouts {
    /// Lays `flat`, a tree's nodes in tree order, out in a screen `size` pixels across and down at `scale` pixels to
    /// the logical pixel, measuring text with `measuring`, and then each node at a place among `tops` as a root of its
    /// own, out of flow in the screen's box, the content of an open popup: each node's boxes, in the same order, and
    /// each scroll's pitch. A node within a popup's content whose root is not among `tops` is laid out nowhere.
    ///
    /// The last layout's boxes when the tree lays out as it did, at the same size, scale, and tops, which it told
    /// ([`Relaid`]).
    pub fn lay_out<'flat>(
        &mut self,
        flat: &'flat [Flat<'flat>],
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        size: Vec2,
        scale: f32,
    ) -> &Laidout {
        let holds = match &self.holds {
            Holds::At(at) => at.is(size, scale, tops),
            Holds::Not => false,
        };
        if holds {
            return &self.laidout;
        }
        self.laidout = self.laid(flat, tops, measuring, size, scale);
        let waited = measuring.waited();
        self.holds = if waited {
            self.nodes.clear();
            Holds::Not
        } else {
            Holds::At(LaidAt { size, scale, tops: tops.to_vec() })
        };

        &self.laidout
    }

    /// Lays `flat` out with Taffy, as [`Self::lay_out`] says, from what was kept of each node that lays out as it did,
    /// and keeps what Taffy kept of each.
    fn laid<'flat>(
        &mut self,
        flat: &'flat [Flat<'flat>],
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        size: Vec2,
        scale: f32,
    ) -> Laidout {
        let rescaled = self.scale.to_bits() != scale.to_bits();
        if rescaled {
            self.nodes.clear();
            self.scale = scale;
        }
        let mut parents = vec![Parent::Other; flat.len()];
        for flat in flat {
            let parent = Parent::of(flat.node.widget().arrange());
            for child in &flat.children {
                parents[*child] = parent;
            }
        }
        let flowing: Vec<Vec<usize>> = flat.iter().map(|node| in_flow(node, flat)).collect();
        let mut was = mem::take(&mut self.nodes);
        let mut keys: Vec<KeyPath> = Vec::with_capacity(flat.len());
        let mut anew = vec![false; flat.len()];
        let mut nodes: Vec<Laying<'_>> = Vec::with_capacity(flat.len());
        for (at, ((node, parent), children)) in flat.iter().zip(parents).zip(&flowing).enumerate() {
            let widget = node.node.widget();
            let arrange = widget.arrange();
            measuring.at(&node.path, Asker::Layout);
            let room = widget.room(node.node.style(), measuring);
            let style = flow(node.node.style(), arrange, parent, room, scale);
            let (key, kept) = match was.remove_entry(&node.path) {
                Some((key, kept)) => (key, kept.moved(at)),
                None => (node.path.clone(), Kept::new(at)),
            };
            anew[at] = kept.cache.is_empty();
            keys.push(key);
            nodes.push(Laying { arrange, flat: node, children: children.clone(), style, kept });
        }
        if let Some(root) = nodes.first_mut() {
            root.style.size = Size { width: Dimension::length(size.x), height: Dimension::length(size.y) };
        }
        for top in tops {
            nodes[*top].style.position = Position::Absolute;
        }
        let reached = reached(&flowing, tops);
        for (at, reached) in reached.iter().enumerate().rev() {
            let holds = flowing[at].iter().any(|child| anew[*child]);
            let unreached = !*reached;
            let relaid = anew[at] || holds || unreached;
            if relaid {
                anew[at] = true;
                let laying = &mut nodes[at].kept;
                let _ = laying.cache.clear();
                laying.layout = Layout::with_order(0);
            }
        }

        let mut tree = Tree { nodes, measuring };
        let space = Size { width: AvailableSpace::Definite(size.x), height: AvailableSpace::Definite(size.y) };
        compute_root_layout(&mut tree, NodeId::from(0_usize), space);
        for top in tops {
            compute_root_layout(&mut tree, NodeId::from(*top), space);
        }

        let layouts: Vec<Layout> = tree.nodes.iter().map(|laying| laying.kept.layout).collect();
        self.nodes = keys.into_iter().zip(tree.nodes).map(|(key, laying)| (key, laying.kept)).collect();

        boxed(flat, tops, &flowing, &layouts, scale)
    }
}

/// A node whose layout changed lets go of what Taffy kept of it and of every node holding it, and the last boxes no
/// longer hold; a node new to the tree takes nothing kept at or within its path, which may be left from a tree before
/// the last, when a tree between was never laid out; and what Taffy kept of a node that left goes at the next layout,
/// which keeps only what its tree holds.
impl Relaid for Layouts {
    fn changed(&mut self, path: &[Segment]) {
        self.holds = Holds::Not;
        for depth in 1..=path.len() {
            if let Some(kept) = self.nodes.get_mut(&path[..depth]) {
                let _ = kept.cache.clear();
            }
        }
    }

    /// What layout keeps holds while only drawing changed.
    fn redrawn(&mut self, _: &[Segment]) {}

    fn entered(&mut self, path: &[Segment]) {
        self.holds = Holds::Not;
        self.nodes.retain(|kept, _| !kept.segments().starts_with(path));
    }

    fn left(&mut self, _: &[Segment]) {
        self.holds = Holds::Not;
    }
}

/// Which of the nodes whose children laid out in their boxes are `flowing` are laid out: the root and each of `tops`,
/// and every node within one laid out in its box.
fn reached(flowing: &[Vec<usize>], tops: &[usize]) -> Vec<bool> {
    let mut reached = vec![false; flowing.len()];
    let mut stack: Vec<usize> = tops.to_vec();
    stack.push(0);
    while let Some(at) = stack.pop() {
        let Some(node) = reached.get_mut(at) else {
            continue;
        };
        *node = true;
        stack.extend_from_slice(&flowing[at]);
    }

    reached
}
