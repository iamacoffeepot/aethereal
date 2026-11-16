//! A block built into widgets (ADR-0056: Markdown), with one stack of the boxes still open, never by recursion: a box
//! is made once each of its children is, so each open box keeps the children made and those still to make.
//!
//! ```text
//! Body ──> a leaf (a text, a code panel, a table, a rule) ──> its node
//!      └─> a box of blocks (a list, its item, a quote) ──> pushed, its children made in turn ──> popped into its node
//! ```

use super::{Body, Images, Inks, Inline, Leads, Look, Numbering};
use crate::input::{Event, Raises};
use crate::style::{Style, Track};
use crate::tree::{Node, Segment};
use crate::widget::Axis;
use crate::widgets::{Column, Flow, Grid, Link, Picture, Rich, Row, Scroll};
use aethereal_render::text::{Colour, Run};

/// What blocks are built with: the style set, the pictures the caller decoded for images by their sources, and the
/// event a link raises.
#[derive(Debug, Clone, Copy)]
pub struct Kit<'kit, E> {
    pub inks: &'kit Inks,
    pub images: &'kit Images,
    pub link: Raises<E>,
}

/// What is still to make in a box: a block, a list's item with its marker, or a column of blocks styled as its box
/// says.
#[derive(Debug, Clone, Copy)]
enum Unit<'body> {
    Block(&'body Body),
    Item { marker: u64, numbering: Numbering, blocks: &'body [Body] },
    Blocks { blocks: &'body [Body], style: &'body Style },
}

/// Which flex box an open box is.
#[derive(Debug, Clone, Copy)]
enum Flex {
    Row,
    Column,
}

/// A box still open: its place among its siblings, which box it is and its style, the children made, and those still
/// to make, the next last.
#[derive(Debug)]
struct Open<'body> {
    segment: Segment,
    flex: Flex,
    style: Style,
    made: Vec<Node>,
    unmade: Vec<(Segment, Unit<'body>)>,
}

impl Open<'_> {
    /// Its node, over the children made.
    fn node(self) -> Node {
        match self.flex {
            Flex::Row => Node::new(self.segment, Row, self.style, self.made),
            Flex::Column => Node::new(self.segment, Column, self.style, self.made),
        }
    }
}

/// What making a unit gives: its node, or a box whose children are still to make.
enum Made<'body> {
    Leaf(Node),
    Box(Open<'body>),
}

/// The node of `body` at `segment`, styled by `inks`, its links raising `link`.
pub fn block<E: Event>(body: &Body, segment: Segment, kit: Kit<'_, E>) -> Node {
    let mut open = match make(Unit::Block(body), segment, kit) {
        Made::Leaf(node) => return node,
        Made::Box(open) => vec![open],
    };
    loop {
        let Some(top) = open.last_mut() else {
            return Node::new(segment, Column, Style::default(), Vec::new());
        };
        if let Some((segment, unit)) = top.unmade.pop() {
            match make(unit, segment, kit) {
                Made::Leaf(node) => top.made.push(node),
                Made::Box(child) => open.push(child),
            }
            continue;
        }
        let Some(finished) = open.pop() else {
            continue;
        };
        let node = finished.node();
        match open.last_mut() {
            Some(parent) => parent.made.push(node),
            None => return node,
        }
    }
}

/// What `unit` at `segment` makes: a leaf's node, or a box with its children to make.
fn make<'body, E: Event>(unit: Unit<'body>, segment: Segment, kit: Kit<'body, E>) -> Made<'body> {
    let Kit { inks, link, .. } = kit;
    match unit {
        Unit::Block(body) => made(body, segment, kit),
        Unit::Item { marker, numbering, blocks } => {
            let marker = match numbering {
                Numbering::Bullets => String::from("\u{2022}"),
                Numbering::From(_) => format!("{marker}."),
            };
            let marker = text(vec![Run::new(&marker, inks.marker.colour)], Vec::new(), Flow::Wraps, link);
            let marker = Node::new(Segment::Place(0), marker, inks.marker.clone(), Vec::new());
            let body = Unit::Blocks { blocks, style: &inks.item_body };

            Made::Box(Open {
                segment,
                flex: Flex::Row,
                style: inks.item.clone(),
                made: vec![marker],
                unmade: vec![(Segment::Place(1), body)],
            })
        }
        Unit::Blocks { blocks, style } => Made::Box(column(segment, style, blocks)),
    }
}

/// What the block `body` at `segment` makes.
fn made<'body, E: Event>(body: &'body Body, segment: Segment, kit: Kit<'body, E>) -> Made<'body> {
    let Kit { inks, images, link } = kit;
    let node = match body {
        Body::Paragraph(inline) => rich(inline, segment, &inks.paragraph, inks, link),
        Body::Heading { level, text } => {
            let at = usize::from(*level).clamp(1, inks.headings.len()) - 1;
            rich(text, segment, &inks.headings[at], inks, link)
        }
        Body::Code { code, .. } => {
            let code = text(vec![Run::new(code, inks.code.colour)], Vec::new(), Flow::Unbroken, link);
            let code = Node::new(Segment::Place(0), code, inks.code.clone(), Vec::new());
            let across = Scroll { axis: Axis::Row, ..Scroll::down() };
            let scroll = Node::new(Segment::Place(0), across, Style::default(), vec![code]);
            Node::new(segment, Column, inks.code_panel.clone(), vec![scroll])
        }
        Body::Table { head, rows } => table(head, rows, segment, inks, link),
        Body::Rule => Node::new(segment, Column, inks.rule.clone(), Vec::new()),
        Body::Image { alt, source } => images.get(source).map_or_else(
            || placeholder(alt, segment, inks, link),
            |pixels| Node::new(segment, Picture { pixels: pixels.clone() }, inks.picture.clone(), Vec::new()),
        ),
        Body::List { numbering, items } => {
            let first = match numbering {
                Numbering::Bullets => 0,
                Numbering::From(first) => *first,
            };
            let mut unmade: Vec<(Segment, Unit<'_>)> = items
                .iter()
                .zip(0_u32..)
                .zip(first..)
                .map(|((blocks, at), marker)| {
                    (Segment::Place(at), Unit::Item { marker, numbering: *numbering, blocks })
                })
                .collect();
            unmade.reverse();
            return Made::Box(Open { segment, flex: Flex::Column, style: inks.list.clone(), made: Vec::new(), unmade });
        }
        Body::Quote(blocks) => {
            let bar = Node::new(Segment::Place(0), Column, inks.quote_bar.clone(), Vec::new());
            let body = Unit::Blocks { blocks, style: &inks.quote_body };
            return Made::Box(Open {
                segment,
                flex: Flex::Row,
                style: inks.quote.clone(),
                made: vec![bar],
                unmade: vec![(Segment::Place(1), body)],
            });
        }
    };

    Made::Leaf(node)
}

/// The alt text `alt` of an image at `segment` whose picture the caller has not decoded, in the image colour.
fn placeholder<E: Event>(alt: &str, segment: Segment, inks: &Inks, link: Raises<E>) -> Node {
    let alt = if alt.is_empty() {
        "image"
    } else {
        alt
    };
    let alt = text(vec![Run::new(alt, inks.image)], Vec::new(), Flow::Wraps, link);

    Node::new(segment, alt, inks.paragraph.clone(), Vec::new())
}

/// A column at `segment` styled `style` of `blocks`, each by its place.
fn column<'body>(segment: Segment, style: &Style, blocks: &'body [Body]) -> Open<'body> {
    let mut unmade: Vec<(Segment, Unit<'_>)> =
        blocks.iter().zip(0_u32..).map(|(body, at)| (Segment::Place(at), Unit::Block(body))).collect();
    unmade.reverse();

    Open { segment, flex: Flex::Column, style: style.clone(), made: Vec::new(), unmade }
}

/// A table at `segment`: a grid of a column for each of its cells across, as wide as their content within the width
/// there is, its header's cells first and then each row's, in order.
fn table<E: Event>(head: &[Inline], rows: &[Vec<Inline>], segment: Segment, inks: &Inks, link: Raises<E>) -> Node {
    let columns = head.len();
    let style = Style { columns: vec![Track::Auto; columns], ..inks.table.clone() };
    let heads = head.iter().map(|cell| (cell, &inks.head_cell));
    let cells = rows.iter().flatten().map(|cell| (cell, &inks.cell));
    let made =
        heads.chain(cells).zip(0_u32..).map(|((cell, style), at)| rich(cell, Segment::Place(at), style, inks, link));

    Node::new(segment, Grid, style, made.collect())
}

/// The text of `inline` at `segment`, styled `style`, each span a run in its look's colour, a link's in the link colour,
/// raising `link` with its target when pressed.
fn rich<E: Event>(inline: &Inline, segment: Segment, style: &Style, inks: &Inks, link: Raises<E>) -> Node {
    let mut runs = Vec::with_capacity(inline.spans.len());
    let mut leading = Vec::new();
    for span in &inline.spans {
        let colour = match span.link {
            Leads::To(_) => inks.link,
            Leads::Nowhere => coloured(span.look, style.colour, inks),
        };
        if let Leads::To(at) = span.link {
            let target = inline.links.get(at as usize).cloned().unwrap_or_default();
            let run = u32::try_from(runs.len()).unwrap_or(u32::MAX);
            leading.push(Link { run, target });
        }
        runs.push(Run::new(&span.text, colour));
    }

    Node::new(segment, text(runs, leading, Flow::Wraps, link), style.clone(), Vec::new())
}

/// The colour a span of `look` is drawn in, plain text in `plain`.
const fn coloured(look: Look, plain: Colour, inks: &Inks) -> Colour {
    match look {
        Look::Plain => plain,
        Look::Emphasis => inks.emphasis,
        Look::Strong => inks.strong,
        Look::Strike => inks.strike,
        Look::Code => inks.inline_code,
        Look::Image => inks.image,
    }
}

/// A text of `runs`, flowing as `flow` says, its `links` raising `link`.
const fn text<E: Event>(runs: Vec<Run>, links: Vec<Link>, flow: Flow, link: Raises<E>) -> Rich<E> {
    Rich { runs, links, flow, link }
}
