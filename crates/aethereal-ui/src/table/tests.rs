//! The widget table: a state made when its key appears, kept while it stays, and dropped when it goes.

use super::Table;
use crate::style::Style;
use crate::tree::{KeyPath, Node, Segment, Shape, Tree};
use crate::widget::{Arrange, Draws, Laid, Widget};
use crate::widgets::Column;
use aethereal_data::{Plain, to_bytes};

/// What a widget of the tests keeps: how often it was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.test.pressed")]
struct Pressed {
    count: u32,
}

/// A widget that keeps a count, labelled `label`, which its view may change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counter {
    label: u32,
}

impl Widget for Counter {
    type State = Pressed;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Pressed, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

impl Table {
    /// Keeps `state` under `path`, as a widget's input will.
    fn set(&mut self, path: &KeyPath, state: Pressed) {
        let at = self.kept.binary_search_by(|kept| kept.path.cmp(path));
        let Ok(at) = at else {
            panic!("a state is kept under {path:?}");
        };
        let Ok(bytes) = to_bytes(&state) else {
            panic!("a count encodes");
        };
        self.kept[at].state = bytes;
    }
}

/// A column over a counter at each of `places`, labelled `label`.
fn counters(places: &[u32], label: u32) -> Tree {
    let children = places
        .iter()
        .map(|place| Node::new(Segment::Place(*place), Counter { label }, Style::default(), Vec::new()))
        .collect();

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), children))
}

/// Refreshes `table` from `tree`.
fn refresh(table: &mut Table, tree: &Tree) {
    let mut shape = Shape::default();
    let Ok(flattened) = tree.flat(&mut shape) else {
        panic!("the tree lays out");
    };
    table.refresh(&flattened.nodes);
}

#[test]
fn a_state_is_kept_while_its_key_stays_and_dropped_once_it_goes() {
    // Catches a table that makes every state anew at each rebuild, so hover and scroll reset every frame; one keyed by
    // a widget's place among what shows, so hiding a sibling before it hands its state to another; and one that keeps
    // a state after its widget goes, so a widget shown again later comes back as it was left.
    let mut table = Table::default();
    let second = KeyPath::of(&[Segment::Place(0), Segment::Place(1)]);
    refresh(&mut table, &counters(&[0, 1], 7));
    table.set(&second, Pressed { count: 3 });

    refresh(&mut table, &counters(&[1], 8));
    let kept = table.state::<Counter>(&second);
    refresh(&mut table, &counters(&[0], 8));
    let gone = table.state::<Counter>(&second);
    refresh(&mut table, &counters(&[0, 1], 8));
    let back = table.state::<Counter>(&second);

    assert_eq!(kept, Some(Pressed { count: 3 }), "kept across a rebuild with its sibling hidden and its label changed");
    assert_eq!(gone, None);
    assert_eq!(back, Some(Pressed::default()), "made anew when the key comes back");
}

#[test]
fn a_state_is_made_anew_when_another_widget_takes_its_key() {
    // Catches a table that hands a widget the state another type left at its key, read as its own.
    let mut table = Table::default();
    let first = KeyPath::of(&[Segment::Place(0), Segment::Place(0)]);
    refresh(&mut table, &counters(&[0], 1));
    table.set(&first, Pressed { count: 3 });

    let column = Node::new(Segment::Place(0), Column, Style::default(), Vec::new());
    refresh(&mut table, &Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![column])));
    refresh(&mut table, &counters(&[0], 1));

    assert_eq!(table.state::<Counter>(&first), Some(Pressed::default()));
}
