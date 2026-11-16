//! Markdown as widgets (ADR-0056: Markdown): a reply's markdown read into its top-level blocks ([`Document`]), and
//! each block built into widgets of the library's own, styled by one style set the caller passes in ([`Prose`]).
//!
//! ```text
//! markdown ──> Document (read again only from its last block as it grows) ──> Blocks, each keyed by its place
//!          ──> Markdown { blocks, prose, link => … }                       ──> a column of every block
//!          └─> Column { items: blocks as block key block.index,
//!                       MarkdownBlock { memo, block: block, prose: …, link => … } } ──> each block kept by memo
//! ```
//!
//! - A paragraph, a heading, and a table's cell are each one [`Rich`](crate::widgets::Rich) text: its emphasis, strong, struck, code, and
//!   linked spans are runs of it, each in its own colour, as the renderer draws a text's runs in colours alone.
//! - A code block is its code in a monospace `Rich`, unbroken, in a [`Scroll`](crate::widgets::Scroll) across, on a panel; its language is
//!   kept in its [`Body`] for a highlighter to read later.
//! - A list is a column of rows, each its marker and a column of its blocks; a quote a row of a bar and a column of its
//!   blocks; a table a [`Grid`](crate::widgets::Grid) of its cells, its columns as wide as their content up to the width there is; a rule an
//!   empty box its style sizes.
//! - A link is drawn in the link colour, and a press and release on it raises the composite's `link` with its target.
//!   Nothing is opened: the caller decides.
//! - An image is its alt text, in the placeholder colour.
//!
//! A reply grows token by token, so each block before its last stays the same shared value as it streams, and a block
//! kept by `memo`, or compared equal in the screen's tree, is neither laid out nor drawn again.

mod build;
mod document;
mod parse;
#[cfg(test)]
mod tests;

pub use document::Document;
pub use parse::MOST_NESTING;

use crate::input::{Event, Raises, Raising};
use crate::style::{Border, Edges, Gap, Length, Space, Style};
use crate::tree::{Composite, Element, ItemKey, Node, Segment};
use crate::widgets::Column;
use aethereal_render::text::{Colour, Font};
use core::ops::Deref;
use core::slice::Iter;
use std::sync::Arc;

/// One top-level block of a document: its place among the document's blocks, by which it is keyed, and what it is,
/// shared, so a block kept as a reply grows is the same value and compares equal at once.
#[derive(Debug, Clone)]
pub struct Block {
    pub index: u32,
    pub body: Arc<Body>,
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        let placed = self.index == other.index;
        let shared = Arc::ptr_eq(&self.body, &other.body);

        placed && (shared || self.body == other.body)
    }
}

/// A document's top-level blocks, in order, shared: the same blocks compare equal at once.
#[derive(Debug, Clone, Default)]
pub struct Blocks(Arc<Vec<Block>>);

impl PartialEq for Blocks {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Deref for Blocks {
    type Target = [Block];

    fn deref(&self) -> &[Block] {
        &self.0
    }
}

impl<'blocks> IntoIterator for &'blocks Blocks {
    type Item = &'blocks Block;
    type IntoIter = Iter<'blocks, Block>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// What a block of markdown is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Paragraph(Inline),
    /// A heading, its level from 1.
    Heading {
        level: u8,
        text: Inline,
    },
    /// A code block: its code, its last line break left out, and the language its fence names, empty for none.
    Code {
        language: String,
        code: String,
    },
    /// A list: each item's blocks.
    List {
        numbering: Numbering,
        items: Vec<Vec<Self>>,
    },
    Quote(Vec<Self>),
    /// A table: its header's cells, and each row's, every row as many as the widest.
    Table {
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
    /// An image on a line of its own: its alt text, and its source, which the library never fetches.
    Image {
        alt: String,
        source: String,
    },
}

/// How a list's items are marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numbering {
    Bullets,
    /// Numbered, the first item this number.
    From(u64),
}

/// A block's text: its spans in order, and the targets of its links, which a span names by their place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub spans: Vec<Span>,
    pub links: Vec<String>,
}

/// A run of a block's text in one look, leading to a link or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub look: Look,
    pub link: Leads,
}

impl Span {
    /// `text`, plain and leading nowhere.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Self { text: String::from(text), look: Look::Plain, link: Leads::Nowhere }
    }
}

/// How a span of text is marked: when marks nest, inline code shows above all, then struck, strong, and emphasised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Plain,
    Emphasis,
    Strong,
    Strike,
    Code,
    /// The alt text of an image within a line of text.
    Image,
}

/// Where a span leads when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leads {
    Nowhere,
    /// To the link at this place among its text's links.
    To(u32),
}

/// The style set markdown is built with, shared, so a block compares its style set with the last at once.
#[derive(Debug, Clone, Default)]
pub struct Prose(Arc<Inks>);

impl Prose {
    /// The style set `inks`.
    #[must_use]
    pub fn new(inks: Inks) -> Self {
        Self(Arc::new(inks))
    }

    /// Its styles.
    #[must_use]
    pub fn inks(&self) -> &Inks {
        &self.0
    }
}

impl PartialEq for Prose {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

/// Every style markdown is built with: a style for each kind of box, and a colour for each look of a span, as a
/// text's runs differ by colour alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Inks {
    /// The column a whole document is: the space between its blocks is its gap.
    pub document: Style,
    /// A paragraph's text, and the font, size, and colour every text starts from.
    pub paragraph: Style,
    /// The headings of levels 1, 2, and 3; deeper ones are styled as 3.
    pub headings: [Style; 3],
    /// A code block's panel, and its code's text.
    pub code_panel: Style,
    pub code: Style,
    /// A list's column, each item's row, its marker, and the column of its blocks.
    pub list: Style,
    pub item: Style,
    pub marker: Style,
    pub item_body: Style,
    /// A quote's row, the bar along its side, and the column of its blocks.
    pub quote: Style,
    pub quote_bar: Style,
    pub quote_body: Style,
    /// A table's grid, its header's cells, and its rows' cells. The grid's columns are set to one of `Auto` for each
    /// column of the table.
    pub table: Style,
    pub head_cell: Style,
    pub cell: Style,
    /// A rule's box: it draws its background.
    pub rule: Style,
    /// The colour of each look of a span, and of a link.
    pub emphasis: Colour,
    pub strong: Colour,
    pub strike: Colour,
    pub inline_code: Colour,
    pub link: Colour,
    pub image: Colour,
}

/// Logical pixels on every side.
const fn padded(pixels: f32) -> Edges<Space> {
    Edges::all(Space::Pixels(pixels))
}

/// A gap of `pixels` between rows and between columns.
const fn gapped(pixels: f32) -> Gap {
    Gap { rows: Space::Pixels(pixels), columns: Space::Pixels(pixels) }
}

impl Default for Inks {
    /// Light text on a dark panel, in the renderer's own fonts, each colour in linear light: what the overlay's chat
    /// reads on.
    fn default() -> Self {
        let text = Colour::opaque(0.82, 0.80, 0.74);
        let paragraph = Style { colour: text, font_size: 15.0, ..Style::default() };
        let heading =
            |size: f32| Style { font_size: size, colour: Colour::opaque(0.95, 0.92, 0.82), ..Style::default() };
        let column = Style { gap: gapped(6.0), ..Style::default() };
        let code =
            Style { font: Font::Mono, font_size: 13.0, colour: Colour::opaque(0.78, 0.86, 0.80), ..Style::default() };

        Self {
            document: Style { gap: gapped(10.0), padding: padded(4.0), ..Style::default() },
            paragraph: paragraph.clone(),
            headings: [heading(24.0), heading(20.0), heading(17.0)],
            code_panel: Style {
                padding: padded(8.0),
                background: Colour { red: 0.02, green: 0.02, blue: 0.025, alpha: 0.9 },
                radius: 4.0,
                ..Style::default()
            },
            code: code.clone(),
            list: column.clone(),
            item: Style { gap: gapped(6.0), ..Style::default() },
            marker: Style { min_width: Length::Pixels(18.0), ..paragraph.clone() },
            item_body: Style { grow: 1.0, basis: Length::Pixels(0.0), ..column.clone() },
            quote: Style { gap: gapped(10.0), ..Style::default() },
            quote_bar: Style {
                width: Length::Pixels(3.0),
                background: Colour::opaque(0.35, 0.33, 0.30),
                ..Style::default()
            },
            quote_body: Style { grow: 1.0, basis: Length::Pixels(0.0), ..column },
            table: Style {
                gap: gapped(1.0),
                background: Colour::opaque(0.06, 0.055, 0.05),
                border: Border { width: 1.0, colour: Colour::opaque(0.06, 0.055, 0.05) },
                ..Style::default()
            },
            head_cell: Style {
                padding: padded(6.0),
                background: Colour::opaque(0.03, 0.028, 0.025),
                colour: Colour::opaque(0.95, 0.92, 0.82),
                ..paragraph.clone()
            },
            cell: Style { padding: padded(6.0), background: Colour::opaque(0.012, 0.011, 0.01), ..paragraph },
            rule: Style {
                height: Length::Pixels(1.0),
                margin: Edges {
                    top: Length::Pixels(4.0),
                    bottom: Length::Pixels(4.0),
                    ..Edges::all(Length::Pixels(0.0))
                },
                background: Colour::opaque(0.35, 0.33, 0.30),
                ..Style::default()
            },
            emphasis: Colour::opaque(0.93, 0.80, 0.55),
            strong: Colour::opaque(1.0, 0.97, 0.88),
            strike: Colour::opaque(0.50, 0.48, 0.45),
            inline_code: code.colour,
            link: Colour::opaque(0.45, 0.70, 1.0),
            image: Colour::opaque(0.60, 0.58, 0.54),
        }
    }
}

/// A whole document as widgets: a column of its blocks, each keyed by its place, styled by `prose`, raising `link`
/// with a link's target when it is pressed. A screen that shows a reply as it streams builds each block on its own
/// with [`MarkdownBlock`] marked `memo`, so only the block that changed is built again.
#[derive(Debug, Clone, PartialEq)]
pub struct Markdown<E> {
    pub blocks: Blocks,
    pub prose: Prose,
    pub link: Raises<E>,
}

/// One block of a document as widgets, styled by `prose`, raising `link` with a link's target when it is pressed: a
/// composite a screen keeps with `memo` while the block stands.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownBlock<E> {
    pub block: Block,
    pub prose: Prose,
    pub link: Raises<E>,
}

impl<E: Event> Raising for Markdown<E> {
    type Event = E;
}

impl<E: Event> Raising for MarkdownBlock<E> {
    type Event = E;
}

impl<E: Event> Element for Markdown<E> {
    fn node(self, segment: Segment, children: Vec<Node>) -> Node {
        let style = self.prose.inks().document.clone();

        self.styled(segment, style, children)
    }

    fn styled(self, segment: Segment, style: Style, _: Vec<Node>) -> Node {
        let inks = self.prose.inks();
        let blocks = self.blocks.iter().map(|block| {
            let key = Segment::Item(ItemKey::from(block.index));
            build::block(&block.body, key, inks, self.link)
        });

        Node::new(segment, Column, style, blocks.collect())
    }
}

impl<E: Event> Element for MarkdownBlock<E> {
    fn node(self, segment: Segment, _: Vec<Node>) -> Node {
        build::block(&self.block.body, segment, self.prose.inks(), self.link)
    }

    /// The block, its outermost box styled `style` in place of its own.
    fn styled(self, segment: Segment, style: Style, children: Vec<Node>) -> Node {
        self.node(segment, children).restyled(style)
    }
}

impl<E: Event> Composite for Markdown<E> {}

impl<E: Event> Composite for MarkdownBlock<E> {}
