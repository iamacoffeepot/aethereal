//! A tree's nodes in tree order, kept from one flattening to the next (ADR-0056: A screen is an actor): each node's
//! path, its place among its parent's children, and the places of its children, so a node that stood at its path in
//! the last flattening takes its path and its list of children from it, and only a node new to the tree makes a path.
//!
//! What a flattening hands on beside the nodes is where each stood in the last ([`Was`]): what a screen keeps of each
//! node in tree order, its layout and its listing, follows its node there, so a list grown by one item moves what is
//! kept of the rest rather than letting it go.
//!
//! ```text
//! last shape ──┐
//! tree ────────┴─> walk in tree order: a child at its parent's last child's key ──> its path and children taken
//!                                      a child new to its parent                ──> a path made
//!              ──> nodes, each beside where it stood (Was), and the shape kept for the next
//! ```

use super::{Flat, Flattened, KeyPath, MOST_DEPTH, Node, Segment, Tree, TreeRefused};
use core::mem;
use std::collections::BTreeSet;

/// What a screen keeps of its tree's last flattening for the next: each node's path, its place among its parent's
/// children as the view built them, and the places of its children, in tree order; and the number of the flattening,
/// counted from one, by which what else is kept in tree order knows whether it stands in this order.
#[derive(Debug, Default)]
pub struct Shape {
    entries: Vec<Shaped>,
    generation: u64,
}

/// One node of a tree's last flattening.
#[derive(Debug)]
struct Shaped {
    path: KeyPath,
    born: usize,
    children: Vec<usize>,
}

impl Shaped {
    /// What tells it from its siblings.
    fn segment(&self) -> Option<Segment> {
        self.path.segments().last().copied()
    }
}

/// Where a node of a flattening stood in the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Was {
    /// At this place: it stood at its path then.
    At(usize),
    /// Nowhere: no node stood at its path.
    New,
}

/// Whether a flattening stands as the last one did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reshaped {
    /// Each node stands where it stood, at the same path, and no other did.
    Same,
    /// Some node came, went, or moved.
    Changed,
}

impl Shape {
    /// The place of the node at `path` in the last flattening, if one stood there.
    pub(crate) fn position(&self, path: &KeyPath) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == *path)
    }
}

/// A node being walked, with what it was met as, and where it stood in the last flattening.
struct Walked<'tree> {
    node: &'tree Node,
    parent: Parent,
    was: Was,
}

/// Where a node being walked was met: as the root, or as a child of the node at `at` in the list, `born` its place
/// among that node's children.
#[derive(Debug, Clone, Copy)]
enum Parent {
    Root,
    At { at: usize, born: usize },
}

impl Tree {
    /// Its nodes in tree order, the order they lay out and draw in, each beside its path and the places of its
    /// children in the list, and where it stood in `shape`, the last flattening, which keeps this one for the next:
    /// walked with a stack of its own, so a tree's depth costs no frames. A node at the path of one in `shape` takes its
    /// path and its list of children from it. An item whose key a sibling before it has is left out, with its
    /// descendants, and named in [`Flattened::twice`]; children whose keys stand in order, as markup's places and most
    /// lists' keys do, share none, and are taken as they are.
    ///
    /// # Errors
    ///
    /// [`TreeRefused::Deep`] for a node past [`MOST_DEPTH`], and [`TreeRefused::Twice`] for two siblings at one place in
    /// the markup, which only a tree built by hand can hold. `shape` then keeps nothing.
    pub(crate) fn flat<'tree>(&'tree self, shape: &'tree mut Shape) -> Result<Flattened<'tree>, TreeRefused> {
        shape.generation += 1;
        let mut olds = mem::take(&mut shape.entries);
        let mut entries: Vec<Shaped> = Vec::with_capacity(olds.len());
        let mut nodes: Vec<&Node> = Vec::with_capacity(olds.len());
        let mut was: Vec<Was> = Vec::with_capacity(olds.len());
        let mut twice = Vec::new();
        let mut matched: Vec<(usize, &Node, Was)> = Vec::new();
        let rooted = olds.first().and_then(Shaped::segment) == Some(self.root.segment);
        let first = if rooted {
            Was::At(0)
        } else {
            Was::New
        };
        let mut stack = vec![Walked { node: &self.root, parent: Parent::Root, was: first }];
        while let Some(Walked { node, parent, was: stood }) = stack.pop() {
            let at = entries.len();
            let (born, path) = match (parent, stood) {
                (Parent::Root, Was::At(old)) => (0, olds[old].path.clone()),
                (Parent::Root, Was::New) => (0, KeyPath::of(&[node.segment])),
                (Parent::At { at: parent, born }, Was::At(old)) => {
                    entries[parent].children.push(at);
                    (born, olds[old].path.clone())
                }
                (Parent::At { at: parent, born }, Was::New) => {
                    entries[parent].children.push(at);
                    (born, entries[parent].path.child(node.segment))
                }
            };
            let deep = path.segments().len() > MOST_DEPTH;
            if deep {
                return Err(TreeRefused::Deep { path });
            }
            let mut children = match stood {
                Was::At(old) => mem::take(&mut olds[old].children),
                Was::New => Vec::new(),
            };
            matched.clear();
            let ordered = node.children.windows(2).all(|pair| pair[0].segment < pair[1].segment);
            let mut cursor = 0;
            if ordered {
                for (born, child) in node.children.iter().enumerate() {
                    let was = beside(&olds, &children, &mut cursor, child.segment);
                    matched.push((born, child, was));
                }
            } else {
                let mut seen = BTreeSet::new();
                for (born, child) in node.children.iter().enumerate() {
                    let fresh = seen.insert(child.segment);
                    match child.segment {
                        _ if fresh => {
                            let was = beside(&olds, &children, &mut cursor, child.segment);
                            matched.push((born, child, was));
                        }
                        Segment::Place(_) => return Err(TreeRefused::Twice { path: path.child(child.segment) }),
                        Segment::Item(_) => twice.push(path.child(child.segment)),
                    }
                }
            }
            while let Some((born, child, was)) = matched.pop() {
                stack.push(Walked { node: child, parent: Parent::At { at, born }, was });
            }
            children.clear();
            entries.push(Shaped { path, born, children });
            nodes.push(node);
            was.push(stood);
        }
        let counted = entries.len() == olds.len();
        let stood = was.iter().enumerate().all(|(at, was)| *was == Was::At(at));
        let reshaped = if counted && stood {
            Reshaped::Same
        } else {
            Reshaped::Changed
        };
        shape.entries = entries;
        let generation = shape.generation;
        let nodes = shape
            .entries
            .iter()
            .zip(nodes)
            .map(|(entry, node)| Flat { path: &entry.path, node, born: entry.born, children: &entry.children })
            .collect();

        Ok(Flattened { nodes, twice, was, reshaped, generation })
    }
}

/// Where the child at `segment` of a node stood in the last flattening, among `children`, the places in `olds` of the
/// children it had there: at the one `cursor` reaches, as children that keep their order are met in step, or at the
/// one of its key past it or before it; or nowhere.
fn beside(olds: &[Shaped], children: &[usize], cursor: &mut usize, segment: Segment) -> Was {
    let next = children.get(*cursor).copied();
    let in_step = next.is_some_and(|old| olds[old].segment() == Some(segment));
    if let (true, Some(old)) = (in_step, next) {
        *cursor += 1;
        return Was::At(old);
    }

    children.iter().copied().find(|old| olds[*old].segment() == Some(segment)).map_or(Was::New, Was::At)
}
