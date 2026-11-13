//! A screen's `Prepare`: when it lays out, and what it sends, read from the orders it returns with each of the
//! renderer's answers handed back as the renderer would make them.

use super::orders::Order;
use super::program::{Made, Program};
use super::*;
use crate::draw::frame_bytes;
use crate::style::{Length, Style};
use crate::tree::{KeyPath, Node, Segment};
use crate::widget::{Arrange, Offer, Stateless, Widget};
use crate::widgets::{Column, Stack, Text};
use aethereal_actor::Answered;
use aethereal_data::{Budget, from_bytes};
use aethereal_render::draw_list::Pointer;
use aethereal_render::font::{Font as FontFile, Metrics};
use aethereal_render::instances::Instanced;
use aethereal_render::pass::Interface;
use aethereal_render::placement::{Placed, PlacementId};
use aethereal_render::text::{Colour, Run, TextMade, TextPlaced};

/// The renderer's own monospace font, whose figures are all one width.
pub(super) const MONO: &[u8] = include_bytes!("../../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// The id the renderer gives under `number`, of whatever it made: an id is its number.
pub(super) fn id<T: Plain>(number: u64) -> T {
    let Ok(id) = from_bytes(&number.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("an id is its number");
    };

    id
}

/// A screen 200 by 100 pixels at a UI scale of 1, whose program is made.
pub(super) fn screen() -> Screen {
    let mut screen = Screen::new(Target::of::<Interface>(Frame::Main(id(0))), Viewport::window(200, 100, 1.0));
    screen.program = Program::Made(Made { mesh: id(1), program: id(2), fills: id(5), uniforms: id(3), white: id(4) });

    screen
}

/// A box 20 by 10 logical pixels, filled with `colour`.
fn filled(colour: Colour) -> Style {
    Style { width: Length::Pixels(20.0), height: Length::Pixels(10.0), background: colour, ..Style::default() }
}

/// A tree of a stack over `children`.
fn stack(children: Vec<Node>) -> Tree {
    Tree::new(Node::new(Segment::Place(0), Stack, Style::default(), children))
}

/// A tree of a column holding one text of `text`, in the renderer's monospace font.
fn label(text: &str) -> Tree {
    let style = Style { font: Font::Mono, ..Style::default() };
    let text = Node::new(Segment::Place(0), Text::new(text), style, Vec::new());

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![text]))
}

/// The orders of `orders`.
pub(super) fn sent(orders: &Orders) -> Vec<Order> {
    orders.orders().to_vec()
}

/// Every order `orders` holds, and those the screen returns as the renderer answers each ask among them, made, until
/// none is left to answer, the asks answered last first when `backwards`: each made thing numbered from 100 on.
pub(super) fn answered(screen: &mut Screen, orders: &Orders, backwards: bool) -> Vec<Order> {
    let mut every = Vec::new();
    let mut waiting: Vec<Order> = sent(orders);
    let mut number = 100;
    while !waiting.is_empty() {
        let order = if backwards {
            waiting.pop()
        } else {
            Some(waiting.remove(0))
        };
        let Some(order) = order else {
            break;
        };
        number += 1;
        let more = match &order {
            Order::Instances { context, .. } => {
                screen.instanced(Answered::Yes(Instanced::Made(id(number))), context.clone())
            }
            Order::Place(_, context) => screen.placed(Answered::Yes(Placed::Made(id(number))), context.clone()),
            Order::Text(_, context) => screen.text_made(Answered::Yes(TextMade::Made(id(number))), context.clone()),
            Order::PlaceText(_, context) => {
                screen.text_placed(Answered::Yes(TextPlaced::Made(id(number))), context.clone())
            }
            _ => Orders::default(),
        };
        waiting.extend(sent(&more));
        every.push(order);
    }

    every
}

/// The last list among `orders`.
pub(super) fn last_list(orders: &[Order]) -> Option<Vec<Entry>> {
    orders.iter().rev().find_map(|order| match order {
        Order::List(entries) => Some(entries.clone()),
        _ => None,
    })
}

/// The monospace font's metrics, as the renderer hands them.
pub(super) fn mono() -> Metrics {
    let Ok(font) = FontFile::read(MONO) else {
        panic!("the renderer's monospace font reads");
    };

    font.metrics().clone()
}

#[test]
fn an_identical_tree_lays_nothing_out_and_sends_nothing() {
    // Catches a screen that lays out and sends its list every frame, which is the cost the comparison exists to save;
    // and one that compares by something other than value, so a tree rebuilt equal looks new.
    let mut screen = screen();
    let red = Node::new(Segment::Place(0), Stack, filled(Colour::opaque(1.0, 0.0, 0.0)), Vec::new());
    let first = screen.prepare(stack(vec![red]), &[]);
    answered(&mut screen, &first.orders, false);

    let red = Node::new(Segment::Place(0), Stack, filled(Colour::opaque(1.0, 0.0, 0.0)), Vec::new());
    let again = screen.prepare(stack(vec![red]), &[]);

    assert_eq!(first.outcome, Outcome::Laid);
    assert_eq!(again.outcome, Outcome::Unchanged);
    assert!(again.orders.is_empty(), "{:?}", again.orders);
}

#[test]
fn a_font_s_metrics_coming_lays_the_same_tree_out_once_more() {
    // Catches a screen that draws a text before its font's metrics come, at a guessed size; one that asks for a font's
    // metrics again every frame while they are coming; and one that, the metrics come, still skips the tree because it
    // is the same, so the text never shows.
    let mut screen = screen();

    let met = screen.prepare(label("Gold: 10"), &[]);
    let waiting = screen.prepare(label("Gold: 10"), &[]);
    screen.metrics_read(Font::Mono, mono(), Vec::new());
    let read = screen.prepare(label("Gold: 10"), &[]);

    assert_eq!(met.outcome, Outcome::Waiting);
    assert_eq!(sent(&met.orders), vec![Order::Metrics(MetricsAsked { font: Font::Mono })]);
    assert_eq!(waiting.outcome, Outcome::Unchanged);
    assert!(waiting.orders.is_empty(), "{:?}", waiting.orders);
    assert_eq!(read.outcome, Outcome::Laid);
    let texts = sent(&read.orders).into_iter().filter(|order| matches!(order, Order::Text(..))).count();
    assert_eq!(texts, 1, "the text is made once its font's metrics are in");
}

#[test]
fn a_resize_lays_the_same_tree_out_once_more_and_writes_the_frame_s_size() {
    // Catches a screen that skips the same tree after the window was resized, so its layout is left at the old size;
    // and one whose program goes on turning pixels into clip space by the old frame's size.
    let mut screen = screen();
    let red = Node::new(Segment::Place(0), Stack, filled(Colour::opaque(1.0, 0.0, 0.0)), Vec::new());
    let prepared = screen.prepare(stack(vec![red]), &[]);
    answered(&mut screen, &prepared.orders, false);

    let resized = screen.resize(Viewport::window(400, 300, 1.0));
    let red = Node::new(Segment::Place(0), Stack, filled(Colour::opaque(1.0, 0.0, 0.0)), Vec::new());
    let again = screen.prepare(stack(vec![red]), &[]);

    let frame = Order::WriteUniforms { uniforms: id(3), bytes: frame_bytes(400, 300) };
    assert_eq!(sent(&resized), vec![frame]);
    assert_eq!(again.outcome, Outcome::Laid);
}

#[test]
fn a_changed_text_is_written_and_not_made_anew() {
    // Catches a screen that makes a new text, and releases the old, for every change of what a label says: a text
    // made anew is laid out and its glyphs found again, and is drawn a frame late.
    let mut screen = screen();
    screen.metrics_read(Font::Mono, mono(), Vec::new());
    let prepared = screen.prepare(label("Gold: 10"), &[]);
    answered(&mut screen, &prepared.orders, false);

    let changed = screen.prepare(label("Gold: 12"), &[]);

    let orders = sent(&changed.orders);
    let made = orders.iter().any(|order| matches!(order, Order::Text(..) | Order::Release(_)));
    assert!(!made, "{orders:?}");
    let written = orders.iter().find_map(|order| match order {
        Order::WriteText(write) => Some(write.runs.clone()),
        _ => None,
    });
    assert_eq!(written, Some(vec![Run::new("Gold: 12", Colour::WHITE)]));
}

#[test]
fn the_list_draws_in_tree_order_whatever_order_the_renderer_answers_in() {
    // Catches a list built in the order the placements came back, so a child answered first draws under one it
    // should cover; and a list sent before every draw in it is made, which drops a draw for a frame.
    let mut screen = screen();
    let under = Node::new(Segment::Place(0), Stack, filled(Colour::opaque(1.0, 0.0, 0.0)), Vec::new());
    let over = Node::new(Segment::Place(1), Stack, filled(Colour::opaque(0.0, 0.0, 1.0)), Vec::new());

    let prepared = screen.prepare(stack(vec![under, over]), &[]);
    let orders = answered(&mut screen, &prepared.orders, true);

    let placed: Vec<(DrawKey, PlacementId)> = screen
        .layers
        .iter()
        .filter_map(|layer| layer.entry().map(|entry| (layer.key().clone(), entry)))
        .map(|(key, entry)| {
            let Entry::Draw { placement, .. } = entry else {
                panic!("a layer is drawn");
            };
            (key, placement)
        })
        .collect();
    let of = |segment| {
        let path = KeyPath::of(&[Segment::Place(0), segment]);
        placed.iter().find(|(key, _)| key.path == path).map(|(_, placement)| *placement)
    };
    let (Some(under), Some(over)) = (of(Segment::Place(0)), of(Segment::Place(1))) else {
        panic!("both children are placed: {placed:?}");
    };
    let lists = orders.iter().filter(|order| matches!(order, Order::List(_))).count();
    assert_eq!(lists, 1, "the list is sent once, when every draw is made");
    let draw = |placement| Entry::Draw { placement, pointer: Pointer::Ignores };
    let drawn: Vec<Entry> = last_list(&orders)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| matches!(entry, Entry::Draw { .. }))
        .collect();
    assert_eq!(drawn, vec![draw(under), draw(over)]);
}

#[test]
fn a_draw_that_goes_is_released_only_once_a_list_without_it_is_sent() {
    // Catches a draw released as the tree that drops it is laid out, while the list the renderer draws by still names
    // it until the draws that replace it are made: the rows of a scroll that slides a page, or a label shown in place of
    // another, draw nothing for a frame or more.
    let mut screen = screen();
    screen.metrics_read(Font::Mono, mono(), Vec::new());
    let at = |place: u32, text: &str| {
        let style = Style { font: Font::Mono, ..Style::default() };
        let label = Node::new(Segment::Place(place), Text::new(text), style, Vec::new());
        Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![label]))
    };
    let first = screen.prepare(at(0, "Gold"), &[]);
    answered(&mut screen, &first.orders, false);

    let second = screen.prepare(at(1, "Silver"), &[]);
    let released_early = sent(&second.orders).iter().any(|order| matches!(order, Order::Release(_)));
    let orders = answered(&mut screen, &second.orders, false);

    assert!(!released_early, "the old label was released before its replacement was made: {:?}", second.orders);
    let placed = orders.iter().position(|order| matches!(order, Order::PlaceText(..)));
    let released = orders.iter().position(|order| matches!(order, Order::Release(_)));
    let after = matches!((placed, released), (Some(placed), Some(released)) if placed < released);
    assert!(after, "the old label is released once its replacement is placed and listed: {orders:?}");
}

/// A swatch: as tall as `tall`, filled with `colour`, which layout does not read.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Swatch {
    tall: f32,
    colour: Colour,
}

impl Widget for Swatch {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn same_layout(&self, other: &Self) -> bool {
        self.tall.to_bits() == other.tall.to_bits()
    }

    fn measure(&self, _: &Style, _: &mut Measuring<'_>, _: Offer) -> Vec2 {
        Vec2::new(0.0, self.tall)
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        draws.quad(Quad {
            rect: laid.rect,
            clip: laid.clip,
            colour: self.colour,
            border_width: 0.0,
            border_colour: self.colour,
            radius: 0.0,
            sample: Sample::Solid,
        });
    }
}

#[test]
fn a_change_only_drawing_reads_is_drawn_in_the_boxes_laid_out_before() {
    // Catches a tree that differs only in what a widget draws taken for the same tree, so the swatch keeps its old
    // colour until something else changes.
    let mut screen = screen();
    let swatch =
        |colour| stack(vec![Node::new(Segment::Place(0), Swatch { tall: 10.0, colour }, Style::default(), Vec::new())]);
    let prepared = screen.prepare(swatch(Colour::opaque(1.0, 0.0, 0.0)), &[]);
    answered(&mut screen, &prepared.orders, false);

    let recoloured = screen.prepare(swatch(Colour::opaque(0.0, 0.0, 1.0)), &[]);

    assert_eq!(recoloured.outcome, Outcome::Laid);
    let written = sent(&recoloured.orders).iter().any(|order| matches!(order, Order::WriteInstances { .. }));
    assert!(written, "the new colour is written: {:?}", recoloured.orders);
}

#[test]
fn a_tree_taken_in_after_input_lays_out_as_it_stands() {
    // Catches a tree a screen's methods rebuilt after input kept without being compared with the one laid out, so the
    // next `Prepare`, building the same tree again, hands back the boxes of the tree before it.
    let mut screen = screen();
    let wide = |width: f32| {
        let style = Style { width: Length::Pixels(width), ..filled(Colour::opaque(1.0, 0.0, 0.0)) };
        stack(vec![Node::new(Segment::Place(0), Stack, style, Vec::new())])
    };
    let prepared = screen.prepare(wide(20.0), &[]);
    answered(&mut screen, &prepared.orders, false);

    screen.rebuilt(wide(40.0));
    let _ = screen.prepare(wide(40.0), &[]);

    let path = KeyPath::of(&[Segment::Place(0), Segment::Place(0)]);
    let placed = screen.unkept.placed.iter().find(|(placed, _)| *placed == path).map(|(_, laid)| laid.rect.width());
    assert_eq!(placed, Some(40.0));
}
