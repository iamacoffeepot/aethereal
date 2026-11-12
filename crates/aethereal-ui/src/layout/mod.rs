//! Laying a tree out (ADR-0056: Layout is Taffy): Taffy's flexbox and grid algorithms, run through its low-level
//! traits over the tree's nodes in tree order, never over Taffy's own tree. The root is the screen's size; every length
//! is the style's times the UI scale; and a leaf measures its content, a text with its font's metrics, as Taffy asks.

#[cfg(test)]
mod tests;

use crate::fonts::Measuring;
use crate::style::{Parent, flow};
use crate::tree::Flat;
use crate::widget::{Arrange, Offer, Offered};
use aethereal_math::{Rect2, Vec2};
use core::slice::Iter;
use taffy::{
    AvailableSpace, Cache, CacheTree, Dimension, Layout, LayoutContainingBlock, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RunMode, Size, TraversePartialTree,
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout, compute_oof_layout,
    compute_root_layout,
};

/// Where layout put one node, in its screen's pixels from the screen's top left: its border box, and its content box,
/// the border box less its border and padding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boxed {
    pub rect: Rect2,
    pub content: Rect2,
}

/// Lays `flat`, a tree's nodes in tree order, out in a screen `size` pixels across and down at `scale` pixels to the
/// logical pixel, measuring text with `measuring`: each node's boxes, in the same order.
pub fn lay_out(flat: &[Flat<'_>], measuring: &mut Measuring<'_>, size: Vec2, scale: f32) -> Vec<Boxed> {
    let mut parents = vec![Parent::Other; flat.len()];
    for flat in flat {
        let stacks = flat.node.widget().arrange() == Arrange::Stack;
        if stacks {
            for child in &flat.children {
                parents[*child] = Parent::Stack;
            }
        }
    }
    let mut nodes: Vec<Laying<'_>> = flat
        .iter()
        .zip(parents)
        .map(|(flat, parent)| {
            let arrange = flat.node.widget().arrange();
            Laying {
                arrange,
                flat,
                style: flow(flat.node.style(), arrange, parent, scale),
                cache: Cache::new(),
                layout: Layout::with_order(0),
                hoisted: Vec::new(),
            }
        })
        .collect();
    if let Some(root) = nodes.first_mut() {
        root.style.size = Size { width: Dimension::length(size.x), height: Dimension::length(size.y) };
    }

    let mut tree = Tree { nodes, measuring };
    let space = Size { width: AvailableSpace::Definite(size.x), height: AvailableSpace::Definite(size.y) };
    compute_root_layout(&mut tree, NodeId::from(0_usize), space);

    let mut boxes: Vec<Boxed> = Vec::with_capacity(flat.len());
    let mut origins = vec![Vec2::ZERO; flat.len()];
    for (at, laying) in tree.nodes.iter().enumerate() {
        let Layout { location, size, border, padding, .. } = laying.layout;
        let origin = origins[at] + Vec2::new(location.x, location.y);
        for child in &flat[at].children {
            origins[*child] = origin;
        }
        let rect = Rect2::from_origin_size(origin, Vec2::new(size.width, size.height));
        let inset_min = Vec2::new(border.left + padding.left, border.top + padding.top);
        let inset_max = Vec2::new(border.right + padding.right, border.bottom + padding.bottom);
        let inner_min = rect.min + inset_min;
        let inner_max = rect.max - inset_max;
        let content =
            Rect2 { min: inner_min, max: Vec2::new(inner_max.x.max(inner_min.x), inner_max.y.max(inner_min.y)) };
        boxes.push(Boxed { rect, content });
    }

    boxes
}

/// One node as Taffy lays it out: how it arranges its children, the node, its style as Taffy reads it, and what Taffy
/// keeps of it as it works.
struct Laying<'flat> {
    arrange: Arrange,
    flat: &'flat Flat<'flat>,
    style: taffy::Style,
    cache: Cache,
    layout: Layout,
    hoisted: Vec<NodeId>,
}

/// A tree's nodes as Taffy walks them, by their places in tree order, with the fonts its leaves measure by.
struct Tree<'flat, 'measuring, 'fonts> {
    nodes: Vec<Laying<'flat>>,
    measuring: &'measuring mut Measuring<'fonts>,
}

impl<'flat> Tree<'flat, '_, '_> {
    /// The node Taffy names `id`.
    fn node(&self, id: NodeId) -> &Laying<'flat> {
        &self.nodes[usize::from(id)]
    }

    /// The node Taffy names `id`, to write.
    fn node_mut(&mut self, id: NodeId) -> &mut Laying<'flat> {
        &mut self.nodes[usize::from(id)]
    }
}

/// The children of a node, as Taffy names them.
struct Children<'a>(Iter<'a, usize>);

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|at| NodeId::from(*at))
    }
}

impl TraversePartialTree for Tree<'_, '_, '_> {
    type ChildIter<'a>
        = Children<'a>
    where
        Self: 'a;

    fn child_ids(&self, parent: NodeId) -> Children<'_> {
        Children(self.node(parent).flat.children.iter())
    }

    fn child_count(&self, parent: NodeId) -> usize {
        self.node(parent).flat.children.len()
    }

    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        NodeId::from(self.node(parent).flat.children[index])
    }
}

impl LayoutPartialTree for Tree<'_, '_, '_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type CustomIdent = String;

    fn get_core_container_style(&self, node: NodeId) -> &taffy::Style {
        &self.node(node).style
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.node_mut(node).layout = *layout;
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, node, inputs, |tree, node, inputs| {
            let arrange = tree.node(node).arrange;
            let mut output = match arrange {
                Arrange::Flex(_) => compute_flexbox_layout(tree, node, inputs),
                Arrange::Grid | Arrange::Stack => compute_grid_layout(tree, node, inputs),
                Arrange::Leaf => {
                    let Tree { nodes, measuring } = tree;
                    let laying = &nodes[usize::from(node)];
                    let measured = |known: Size<Option<f32>>, available: Size<AvailableSpace>| {
                        let offer = Offer {
                            width: offered(known.width, available.width),
                            height: offered(known.height, available.height),
                        };
                        let content = laying.flat.node.widget().measure(laying.flat.node.style(), measuring, offer);

                        Size { width: content.x, height: content.y }
                    };
                    compute_leaf_layout(inputs, &laying.style, |_, _| 0.0, measured)
                }
            };
            let performs = inputs.run_mode == RunMode::PerformLayout;
            if performs {
                compute_oof_layout(tree, node, &mut output);
            }

            output
        })
    }
}

/// The space offered along one axis: exactly its known size, or as Taffy offers it.
fn offered(known: Option<f32>, available: AvailableSpace) -> Offered {
    match (known, available) {
        (Some(exactly), _) => Offered::Exactly(exactly),
        (None, AvailableSpace::Definite(most)) => Offered::AtMost(most),
        (None, AvailableSpace::MinContent) => Offered::MinContent,
        (None, AvailableSpace::MaxContent) => Offered::MaxContent,
    }
}

impl LayoutContainingBlock for Tree<'_, '_, '_> {
    type OofItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_oof_item_style(&self, node: NodeId) -> &taffy::Style {
        &self.node(node).style
    }

    fn clear_hoisted_children(&mut self, node: NodeId) {
        self.node_mut(node).hoisted.clear();
    }

    fn add_hoisted_children(&mut self, node: NodeId, hoisted: &[NodeId]) {
        self.node_mut(node).hoisted.extend_from_slice(hoisted);
    }
}

impl CacheTree for Tree<'_, '_, '_> {
    fn cache_get(&mut self, node: NodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        self.node_mut(node).cache.get(inputs)
    }

    fn cache_store(&mut self, node: NodeId, inputs: &LayoutInput, output: LayoutOutput) {
        self.node_mut(node).cache.store(inputs, output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.node_mut(node).cache.clear();
    }
}

impl LayoutFlexboxContainer for Tree<'_, '_, '_> {
    type FlexboxContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> &taffy::Style {
        &self.node(node).style
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> &taffy::Style {
        &self.node(child).style
    }
}

impl LayoutGridContainer for Tree<'_, '_, '_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node: NodeId) -> &taffy::Style {
        &self.node(node).style
    }

    fn get_grid_child_style(&self, child: NodeId) -> &taffy::Style {
        &self.node(child).style
    }
}
