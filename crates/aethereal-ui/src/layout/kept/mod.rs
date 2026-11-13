//! What a screen keeps of its layouts from one to the next (ADR-0056: Layout is Taffy; *settled by the library's
//! layout-cost stage*): the last boxes, handed back while its tree lays out as it did, and what Taffy keeps of each
//! node, so a layout lays out again only the nodes whose layout changed and those that hold them, as Taffy's own tree
//! does when a node is marked dirty.

#[cfg(test)]
mod tests;

use super::{Converting, Laidout, Laying, Stands, Tree, boxed, in_flow};
use crate::fonts::{Asker, Measuring};
use crate::style::Parent;
use crate::tree::{Flattened, Reshaped, Was};
use aethereal_math::Vec2;
use core::cell::OnceCell;
use core::mem;
use taffy::{AvailableSpace, Cache, Layout, LayoutInput, LayoutOutput, NodeId, Size, compute_root_layout};

/// What a screen keeps of its layouts from one to the next (*settled by the library's layout-cost stage*): what Taffy
/// kept of each node, in the tree order of the flattening it was laid out from, and the boxes of the last layout, with
/// whether they hold.
///
/// A layout whose tree lays out as the last one did, at the same size, scale, and open popups, is the last one: its
/// boxes are handed back, and Taffy does not run. Otherwise Taffy runs, and a node whose layout is kept, whose own
/// settings and whose children's laid out as they did, answers from what it kept, so only what changed and what holds
/// it is laid out again. What it kept follows the node to where it stands in the new flattening ([`Was`]), and holds
/// while the node has laid out as it did since the build that layout was of ([`Stamps`]); a node new to the tree, or
/// one whose cache is empty, is laid out again with all that holds it. Every node kept is let go at another UI scale,
/// which every length and measure reads, and after a layout that met a font whose metrics had not come. A node's style
/// is converted to Taffy's only as Taffy first reads it, so a node within one that answers from its cache is never
/// converted: Taffy's style holds a pointer, so it is not kept in a screen, which moves between threads.
#[derive(Debug, Default)]
pub struct Layouts {
    /// What Taffy kept of each node, in the tree order of the flattening numbered `generation`.
    nodes: Vec<Kept>,
    generation: u64,
    /// The UI scale the nodes kept were laid out at.
    scale: f32,
    /// The boxes the last layout gave.
    laidout: Laidout,
    holds: Holds,
}

/// Whether the boxes a screen's last layout gave hold for the tree it lays out next.
#[derive(Debug, Default)]
enum Holds {
    /// They do not: nothing was laid out, or a font's metrics had not come.
    #[default]
    Not,
    /// They do while the tree lays out as it did, at what this says.
    At(LaidAt),
}

/// What a layout was laid out at beside its tree: the build of the tree, the screen's size and UI scale, and the places
/// of the open popups' content.
#[derive(Debug)]
struct LaidAt {
    built: u64,
    size: Vec2,
    scale: f32,
    tops: Vec<usize>,
}

impl LaidAt {
    /// Whether it holds for a tree that last laid out otherwise at the build `relaid`, standing as `reshaped` says, at
    /// `size`, `scale`, and `tops`.
    fn holds(&self, relaid: u64, reshaped: Reshaped, size: Vec2, scale: f32, tops: &[usize]) -> bool {
        let unchanged = relaid <= self.built;
        let standing = reshaped == Reshaped::Same;
        let sized = self.size == size;
        let scaled = self.scale.to_bits() == scale.to_bits();
        let topped = self.tops == tops;

        unchanged && standing && sized && scaled && topped
    }
}

/// What Taffy keeps of one node from one layout to the next: its place in tree order when it was kept, which Taffy
/// names it by; Taffy's cache of what it laid out, boxed, as it is far larger than the rest and what is kept of a node
/// moves with it each layout its tree's order changes; where its parent laid it; the boxes out of flow it holds, by
/// their places; and the build of the tree it was laid out in, none for a node not laid out.
#[derive(Debug)]
pub(super) struct Kept {
    at: usize,
    pub(super) cache: Cached,
    pub(super) layout: Layout,
    pub(super) hoisted: Vec<NodeId>,
    built: u64,
}

/// Taffy's cache of what it laid out of a node: nothing yet, or what it stored, boxed.
#[derive(Debug)]
pub(super) enum Cached {
    Empty,
    Held(Box<Cache>),
}

impl Cached {
    /// What Taffy stored for `inputs`, if it stored it.
    pub(super) fn get(&mut self, inputs: &LayoutInput) -> Option<LayoutOutput> {
        match self {
            Self::Empty => None,
            Self::Held(cache) => cache.get(inputs),
        }
    }

    /// Stores `output` for `inputs`.
    pub(super) fn store(&mut self, inputs: &LayoutInput, output: LayoutOutput) {
        match self {
            Self::Empty => {
                let mut cache = Box::new(Cache::new());
                cache.store(inputs, output);
                *self = Self::Held(cache);
            }
            Self::Held(cache) => cache.store(inputs, output),
        }
    }

    /// Lets go of everything stored, keeping the room it was stored in.
    pub(super) fn clear(&mut self) {
        if let Self::Held(cache) = self {
            let _ = cache.clear();
        }
    }

    /// Whether nothing is stored.
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Held(cache) => cache.is_empty(),
        }
    }
}

impl Kept {
    /// A node at `at`, laid out nowhere yet.
    fn new(at: usize) -> Self {
        Self { at, cache: Cached::Empty, layout: Layout::with_order(0), hoisted: Vec::new(), built: 0 }
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
    /// Lays `flattened`, a tree's nodes in tree order, of the build `built`, which last laid out otherwise at the build
    /// `relaid`, out in a screen `size` pixels across and down at `scale` pixels to the logical pixel, measuring text
    /// with `measuring`, and then each node at a place among `tops` as a root of its own, out of flow in the screen's
    /// box, the content of an open popup: each node's boxes, in the same order, and each scroll's pitch. A node within a
    /// popup's content whose root is not among `tops` is laid out nowhere.
    ///
    /// The last layout's boxes when the tree lays out as it did, at the same size, scale, and tops.
    #[allow(clippy::too_many_arguments)]
    pub fn lay_out<'flat>(
        &mut self,
        flattened: &'flat Flattened<'flat>,
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        (size, scale): (Vec2, f32),
        (built, relaid): (u64, u64),
    ) -> &Laidout {
        let holds = match &self.holds {
            Holds::At(at) => at.holds(relaid, flattened.reshaped, size, scale, tops),
            Holds::Not => false,
        };
        if holds {
            self.generation = flattened.generation;
            return &self.laidout;
        }
        self.laidout = self.laid(flattened, tops, measuring, size, scale, built);
        let waited = measuring.waited();
        self.holds = if waited {
            self.nodes.clear();
            Holds::Not
        } else {
            Holds::At(LaidAt { built, size, scale, tops: tops.to_vec() })
        };

        &self.laidout
    }

    /// Lays `flattened` out with Taffy, as [`Self::lay_out`] says, from what was kept of each node that lays out as it
    /// did, and keeps what Taffy kept of each.
    fn laid<'flat>(
        &mut self,
        flattened: &'flat Flattened<'flat>,
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        size: Vec2,
        scale: f32,
        built: u64,
    ) -> Laidout {
        let flat = flattened.nodes.as_slice();
        let rescaled = self.scale.to_bits() != scale.to_bits();
        let following = self.generation + 1 == flattened.generation;
        let mut was = mem::take(&mut self.nodes);
        if rescaled || !following {
            was.clear();
            self.scale = scale;
        }
        self.generation = flattened.generation;
        let mut parents = vec![Parent::Other; flat.len()];
        for flat in flat {
            let parent = Parent::of(flat.node.widget().arrange());
            for child in flat.children {
                parents[*child] = parent;
            }
        }
        let flowing: Vec<&[usize]> = flat.iter().map(|node| in_flow(node, flat)).collect();
        let mut anew = vec![false; flat.len()];
        let mut nodes: Vec<Laying<'_>> = Vec::with_capacity(flat.len());
        for (at, ((node, parent), children)) in flat.iter().zip(parents).zip(&flowing).enumerate() {
            let widget = node.node.widget();
            let arrange = widget.arrange();
            measuring.at(node.path, Asker::Layout);
            let room = widget.room(node.node.style(), measuring);
            let mut kept = match flattened.was.get(at) {
                Some(Was::At(old)) if *old < was.len() => mem::replace(&mut was[*old], Kept::new(at)).moved(at),
                Some(Was::At(_) | Was::New) | None => Kept::new(at),
            };
            let holds = node.node.stamps().laid <= kept.built;
            if !holds {
                kept.cache.clear();
            }
            anew[at] = kept.cache.is_empty();
            let stands = if at == 0 {
                Stands::Root(size)
            } else if tops.contains(&at) {
                Stands::Top
            } else {
                Stands::Flowing
            };
            let converting = Converting { arrange, parent, room, scale, stands };
            nodes.push(Laying { arrange, flat: node, children, converting, style: OnceCell::new(), kept });
        }
        let reached = reached(&flowing, tops);
        for (at, reached) in reached.iter().enumerate().rev() {
            let holds = flowing[at].iter().any(|child| anew[*child]);
            let unreached = !*reached;
            let relaid = anew[at] || holds || unreached;
            if relaid {
                anew[at] = true;
                let laying = &mut nodes[at].kept;
                laying.cache.clear();
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
        self.nodes = tree
            .nodes
            .into_iter()
            .map(|laying| {
                let mut kept = laying.kept;
                kept.built = built;
                kept
            })
            .collect();

        boxed(flat, tops, &flowing, &layouts, scale)
    }
}

/// Which of the nodes whose children laid out in their boxes are `flowing` are laid out: the root and each of `tops`,
/// and every node within one laid out in its box.
fn reached(flowing: &[&[usize]], tops: &[usize]) -> Vec<bool> {
    let mut reached = vec![false; flowing.len()];
    let mut stack: Vec<usize> = tops.to_vec();
    stack.push(0);
    while let Some(at) = stack.pop() {
        let Some(node) = reached.get_mut(at) else {
            continue;
        };
        *node = true;
        stack.extend_from_slice(flowing[at]);
    }

    reached
}
