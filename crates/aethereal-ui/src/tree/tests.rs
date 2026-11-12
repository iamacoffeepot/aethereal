//! A tree's nodes in tree order, and the trees refused before they are laid out.

use super::{KeyPath, MOST_DEPTH, Node, Segment, Tree, TreeRefused};
use crate::style::Style;
use crate::widgets::Column;

/// A column at `place` over `children`.
fn column(place: u32, children: Vec<Node>) -> Node {
    Node::new(Segment::Place(place), Column, Style::default(), children)
}

#[test]
fn nodes_come_in_tree_order_each_with_its_path_and_its_children() {
    // Catches a walk that hands children before their parent, or in reverse, which would draw a later sibling under an
    // earlier one; and children counted from the wrong place in the list, which would lay out one node's children
    // under another.
    let tree = Tree::new(column(0, vec![column(0, vec![column(4, Vec::new())]), column(2, Vec::new())]));

    let Ok(flat) = tree.flat() else {
        panic!("the tree is walked");
    };

    let paths: Vec<KeyPath> = flat.iter().map(|flat| flat.path.clone()).collect();
    let place = |segments: &[u32]| KeyPath::of(&segments.iter().map(|at| Segment::Place(*at)).collect::<Vec<_>>());
    assert_eq!(paths, vec![place(&[0]), place(&[0, 0]), place(&[0, 0, 4]), place(&[0, 2])]);
    let children: Vec<Vec<usize>> = flat.iter().map(|flat| flat.children.clone()).collect();
    assert_eq!(children, vec![vec![1, 3], vec![2], vec![], vec![]]);
}

#[test]
fn a_tree_with_two_siblings_of_one_segment_or_too_deep_is_refused() {
    // Catches two siblings given one key, which would share one widget state and overwrite each other's draws; and a
    // tree deeper than the cap, which Taffy, which recurses, would lay out until the stack ran out.
    let twice = Tree::new(column(0, vec![column(1, Vec::new()), column(1, Vec::new())]));
    let mut deep = column(0, Vec::new());
    for _ in 0..MOST_DEPTH {
        deep = column(0, vec![deep]);
    }

    let refused = twice.flat().err();
    let too_deep = Tree::new(deep).flat().err();

    let path = KeyPath::of(&[Segment::Place(0), Segment::Place(1)]);
    assert_eq!(refused, Some(TreeRefused::Twice { path }));
    assert!(matches!(too_deep, Some(TreeRefused::Deep { .. })), "{too_deep:?}");
}
