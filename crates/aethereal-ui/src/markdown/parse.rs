//! Markdown read into blocks (ADR-0056: Markdown): `pulldown-cmark`'s events, a flat stream of starts, ends, and
//! text, folded into [`Body`] values with one stack of the containers open, never by recursion. Claude's replies are
//! untrusted, so nothing here panics on any input: an unmatched end is let go, and containers nested past
//! [`MOST_NESTING`] are read as plain text in the container at that depth.
//!
//! ```text
//! source ──> pulldown-cmark events ──> open containers (a stack) ──> each top-level block, and where it starts
//! ```

use super::{Body, Inline, Leads, Look, Numbering, Span};
use core::mem;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// How deep containers, lists, their items, quotes, and tables, may nest: one nested deeper is read as plain text,
/// each of its blocks on a line of its own, in the paragraph of the container at this depth. Each level is a few levels
/// of the screen's tree, which is at most `MOST_DEPTH` deep.
pub const MOST_NESTING: usize = 8;

/// What a document's markdown is read with: the tables and strikethrough of GitHub's markdown, beside `CommonMark`.
const OPTIONS: Options = Options::ENABLE_TABLES.union(Options::ENABLE_STRIKETHROUGH);

/// A document read: its top-level blocks, each with the byte it starts at in what was read, and whether it defines a
/// link by reference, which a block before it may use.
#[derive(Debug, Default)]
pub struct Read {
    pub blocks: Vec<(usize, Body)>,
    pub defines: bool,
}

/// `source` read into its top-level blocks.
pub fn read(source: &str) -> Read {
    let parser = Parser::new_ext(source, OPTIONS);
    let defines = parser.reference_definitions().iter().next().is_some();
    let mut reading = Reading::default();
    for (event, range) in parser.into_offset_iter() {
        reading.event(event, range.start);
    }

    Read { blocks: reading.finish(), defines }
}

/// A container open while the document is read, with the blocks read into it so far.
#[derive(Debug)]
enum Open {
    Quote(Vec<Body>),
    List { numbering: Numbering, items: Vec<Vec<Body>> },
    Item(Vec<Body>),
    Table { head: Vec<Inline>, rows: Vec<Vec<Inline>>, row: Vec<Inline> },
}

/// The block being read, whose text the events between its start and its end hold.
#[derive(Debug, Default)]
enum Leaf {
    /// None: text met now opens a paragraph, as a tight list's items hold their text without one.
    #[default]
    Closed,
    Paragraph(Text),
    Heading {
        level: u8,
        text: Text,
    },
    Code {
        language: String,
        code: String,
    },
    Cell(Text),
}

/// A block's text being read: its spans, the links they lead to, the images among them, how many of each mark are open
/// around what comes next, the links open, and the image whose alt text is being read.
#[derive(Debug, Default)]
struct Text {
    inline: Inline,
    /// Each image read, by the span that holds its alt text, and its source.
    images: Vec<(usize, String)>,
    emphasis: u32,
    strong: u32,
    strike: u32,
    open_links: Vec<u32>,
    image: Imaging,
}

/// Whether a text is reading an image's alt text.
#[derive(Debug, Default)]
enum Imaging {
    #[default]
    Outside,
    /// Inside the image of `source`, whose alt text begins at the span `from`.
    Alt { source: String, from: usize },
}

/// What a text held once read: its spans and links, and the one image it is, when an image is all it holds.
enum Finished {
    Inline(Inline),
    Image { alt: String, source: String },
}

impl Text {
    /// The look of text added now: code above all, then a struck, strong, or emphasised run, and plain.
    const fn look(&self, code: bool) -> Look {
        if code {
            return Look::Code;
        }
        if self.strike > 0 {
            return Look::Strike;
        }
        if self.strong > 0 {
            return Look::Strong;
        }
        if self.emphasis > 0 {
            return Look::Emphasis;
        }

        Look::Plain
    }

    /// Adds `text`, as code when `code` says, to the link open innermost.
    fn push(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let look = self.look(code);
        let link = self.leads();
        let first = match self.image {
            Imaging::Outside => 0,
            Imaging::Alt { from, .. } => from,
        };
        let spans = &mut self.inline.spans;

        // A run continues the span before it when it looks alike, so a text broken by soft breaks is one run; an
        // image's alt text never continues the text before the image, as it is taken out whole when the image ends.
        if let Some(last) = spans.get_mut(first..).and_then(<[Span]>::last_mut) {
            let alike = last.look == look;
            let led = last.link == link;
            if alike && led {
                last.text.push_str(text);
                return;
            }
        }
        spans.push(Span { text: String::from(text), look, link });
    }

    /// Where text added now leads: to the link open innermost.
    fn leads(&self) -> Leads {
        self.open_links.last().map_or(Leads::Nowhere, |link| Leads::To(*link))
    }

    /// Opens an image of `source`: its alt text is read until it ends.
    fn image(&mut self, source: String) {
        self.image = Imaging::Alt { source, from: self.inline.spans.len() };
    }

    /// Closes the image open: its alt text becomes one placeholder span, kept with its source as the image.
    fn imaged(&mut self) {
        let Imaging::Alt { source, from } = mem::take(&mut self.image) else {
            return;
        };
        let from = from.min(self.inline.spans.len());
        let alt: String = self.inline.spans.drain(from..).map(|span| span.text).collect();
        self.images.push((self.inline.spans.len(), source));
        let link = self.leads();
        self.inline.spans.push(Span { text: alt, look: Look::Image, link });
    }

    /// What it held: the image it is, when it holds one image and nothing else but space, or its spans.
    fn finish(mut self) -> Finished {
        self.imaged();
        let inline = self.inline;
        let shown = inline.spans.iter().filter(|span| !span.text.trim().is_empty() || span.look == Look::Image).count();
        let one_shown = shown == 1;
        let one_image = self.images.len() == 1;
        let alone = one_shown && one_image;

        // An image on its own line, with nothing but space beside it, is a block of its own, which a caller may show
        // the picture of.
        if alone && let Some((at, source)) = self.images.pop() {
            let alt = inline.spans.into_iter().nth(at).map(|span| span.text).unwrap_or_default();
            return Finished::Image { alt, source };
        }

        Finished::Inline(inline)
    }
}

/// What reading has made so far: the top-level blocks with where each starts, the containers open innermost last, the
/// block being read, how many containers past [`MOST_NESTING`] are open and read as text, and where the top-level
/// block being read starts.
#[derive(Debug, Default)]
struct Reading {
    blocks: Vec<(usize, Body)>,
    open: Vec<Open>,
    leaf: Leaf,
    flat: usize,
    start: usize,
}

impl Reading {
    /// Takes in one event, which starts at the byte `at` of what is read.
    fn event(&mut self, event: Event<'_>, at: usize) {
        // An event that starts something with nothing open starts a top-level block, which is read again from there.
        let nothing_open = self.open.is_empty();
        let unflattened = self.flat == 0;
        let leaf_closed = matches!(self.leaf, Leaf::Closed);
        let starts = matches!(event, Event::Start(_) | Event::Rule | Event::Html(_) | Event::Text(_));
        if nothing_open && unflattened && leaf_closed && starts {
            self.start = at;
        }
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(end) => self.end_tag(end),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => self.text(&text, false),
            Event::Code(code) => self.text(&code, true),
            Event::InlineMath(math) | Event::DisplayMath(math) => self.text(&math, true),
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => self.rule(),
            Event::FootnoteReference(name) => self.text(&format!("[^{name}]"), false),
            Event::TaskListMarker(done) => {
                let marker = if done {
                    "[x] "
                } else {
                    "[ ] "
                };
                self.text(marker, false);
            }
        }
    }

    /// Opens what `tag` starts.
    fn start_tag(&mut self, tag: Tag<'_>) {
        if self.flat > 0 {
            self.start_flat(&tag);
            return;
        }
        match tag {
            Tag::Paragraph | Tag::HtmlBlock => self.open_leaf(Leaf::Paragraph(Text::default())),
            Tag::Heading { level, .. } => {
                let level = u8::try_from(level as usize).unwrap_or(u8::MAX);
                self.open_leaf(Leaf::Heading { level, text: Text::default() });
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(language) => language.split_whitespace().next().unwrap_or("").to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.open_leaf(Leaf::Code { language, code: String::new() });
            }
            Tag::BlockQuote(_) => self.open_container(Open::Quote(Vec::new())),
            Tag::List(first) => {
                let numbering = first.map_or(Numbering::Bullets, Numbering::From);
                self.open_container(Open::List { numbering, items: Vec::new() });
            }
            Tag::Item => self.open_container(Open::Item(Vec::new())),
            Tag::Table(_) => {
                self.open_container(Open::Table { head: Vec::new(), rows: Vec::new(), row: Vec::new() });
            }
            Tag::TableCell => self.open_leaf(Leaf::Cell(Text::default())),
            Tag::Emphasis => self.mark(|text| text.emphasis += 1),
            Tag::Strong => self.mark(|text| text.strong += 1),
            Tag::Strikethrough => self.mark(|text| text.strike += 1),
            Tag::Link { dest_url, .. } => self.mark(|text| {
                let index = u32::try_from(text.inline.links.len()).unwrap_or(u32::MAX);
                text.inline.links.push(dest_url.into_string());
                text.open_links.push(index);
            }),
            Tag::Image { dest_url, .. } => self.mark(|text| text.image(dest_url.into_string())),
            Tag::TableHead
            | Tag::TableRow
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    /// Closes what `end` ends.
    fn end_tag(&mut self, end: TagEnd) {
        if self.flat > 0 {
            self.end_flat(end);
            return;
        }
        match end {
            TagEnd::Paragraph | TagEnd::HtmlBlock | TagEnd::Heading(_) | TagEnd::CodeBlock => self.close_leaf(),
            TagEnd::TableCell => {
                let leaf = mem::take(&mut self.leaf);
                let cell = match leaf {
                    Leaf::Cell(text) => text.finish(),
                    other => {
                        self.leaf = other;
                        self.close_leaf();
                        Finished::Inline(Inline::default())
                    }
                };
                if let Some(Open::Table { row, .. }) = self.open.last_mut() {
                    row.push(inline_of(cell));
                }
            }
            TagEnd::TableHead => {
                if let Some(Open::Table { head, row, .. }) = self.open.last_mut() {
                    *head = mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Open::Table { rows, row, .. }) = self.open.last_mut() {
                    rows.push(mem::take(row));
                }
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::Table => self.close_container(),
            TagEnd::Emphasis => self.mark(|text| text.emphasis = text.emphasis.saturating_sub(1)),
            TagEnd::Strong => self.mark(|text| text.strong = text.strong.saturating_sub(1)),
            TagEnd::Strikethrough => self.mark(|text| text.strike = text.strike.saturating_sub(1)),
            TagEnd::Link => self.mark(|text| {
                text.open_links.pop();
            }),
            TagEnd::Image => self.mark(Text::imaged),
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    /// A start met inside a container read as text: a container counts one deeper, and a block starts a new line.
    fn start_flat(&mut self, tag: &Tag<'_>) {
        if matches!(tag, Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::Table(_)) {
            self.flat += 1;
        }
        if matches!(
            tag,
            Tag::Paragraph | Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::Item | Tag::TableRow | Tag::HtmlBlock
        ) {
            self.line();
        }
    }

    /// An end met inside a container read as text: the outermost such container's closes the paragraph it was read
    /// into.
    fn end_flat(&mut self, end: TagEnd) {
        if !matches!(end, TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::Table) {
            return;
        }
        self.flat -= 1;
        if self.flat == 0 {
            self.close_leaf();
        }
    }

    /// Starts a new line in the paragraph a container read as text is read into, unless it is empty.
    fn line(&mut self) {
        if let Leaf::Paragraph(text) = &mut self.leaf
            && !text.inline.spans.is_empty()
        {
            text.push("\n", false);
        }
    }

    /// Changes the marks of the text being read, opening a paragraph for it when none is.
    fn mark(&mut self, change: impl FnOnce(&mut Text)) {
        if let Some(text) = self.text_leaf() {
            change(text);
        }
    }

    /// The text being read: the open block's, or a new paragraph's when the open block is code or none is open.
    fn text_leaf(&mut self) -> Option<&mut Text> {
        if matches!(self.leaf, Leaf::Closed) {
            self.leaf = Leaf::Paragraph(Text::default());
        }
        match &mut self.leaf {
            Leaf::Paragraph(text) | Leaf::Heading { text, .. } | Leaf::Cell(text) => Some(text),
            Leaf::Code { .. } | Leaf::Closed => None,
        }
    }

    /// Adds `text` to what is being read: to a code block's code as it is, otherwise as a span, `code` saying whether
    /// it is inline code.
    fn text(&mut self, text: &str, code: bool) {
        if let Leaf::Code { code: kept, .. } = &mut self.leaf {
            kept.push_str(text);
            return;
        }
        if let Some(open) = self.text_leaf() {
            open.push(text, code);
        }
    }

    /// Adds a rule: a line of its own in text read flat.
    fn rule(&mut self) {
        if self.flat > 0 {
            self.line();
            return;
        }
        self.close_leaf();
        self.add(Body::Rule);
    }

    /// Opens `leaf`, closing the one open.
    fn open_leaf(&mut self, leaf: Leaf) {
        self.close_leaf();
        self.leaf = leaf;
    }

    /// Closes the block being read into the container open innermost, or the document.
    fn close_leaf(&mut self) {
        let body = match mem::take(&mut self.leaf) {
            Leaf::Closed => return,
            Leaf::Paragraph(text) | Leaf::Cell(text) => match text.finish() {
                Finished::Inline(inline) => Body::Paragraph(inline),
                Finished::Image { alt, source } => Body::Image { alt, source },
            },
            Leaf::Heading { level, text } => Body::Heading { level, text: inline_of(text.finish()) },
            Leaf::Code { language, mut code } => {
                // A block's last line ends in a line break, which shows as an empty line below it.
                if code.ends_with('\n') {
                    code.pop();
                }
                Body::Code { language, code }
            }
        };
        if !matches!(&body, Body::Paragraph(inline) if inline.spans.is_empty()) {
            self.add(body);
        }
    }

    /// Opens `container`, or, nested past [`MOST_NESTING`], reads it as text into a paragraph.
    fn open_container(&mut self, container: Open) {
        if self.open.len() >= MOST_NESTING {
            self.flat = 1;
            if matches!(self.leaf, Leaf::Closed) {
                self.leaf = Leaf::Paragraph(Text::default());
            }
            self.line();
            return;
        }
        self.close_leaf();
        self.open.push(container);
    }

    /// Closes the container open innermost into the one around it, or the document.
    fn close_container(&mut self) {
        self.close_leaf();
        let Some(closed) = self.open.pop() else {
            return;
        };
        match closed {
            Open::Item(blocks) => match self.open.last_mut() {
                Some(Open::List { items, .. }) => items.push(blocks),
                _ => {
                    for block in blocks {
                        self.add(block);
                    }
                }
            },
            Open::Quote(blocks) => self.add(Body::Quote(blocks)),
            Open::List { numbering, items } => self.add(Body::List { numbering, items }),
            Open::Table { head, mut rows, row } => {
                if !row.is_empty() {
                    rows.push(row);
                }
                let columns = rows.iter().map(Vec::len).chain([head.len()]).max().unwrap_or(0);
                let mut head = head;
                head.resize_with(columns, Inline::default);
                for row in &mut rows {
                    row.resize_with(columns, Inline::default);
                }
                self.add(Body::Table { head, rows });
            }
        }
    }

    /// Adds `body` to the container open innermost, or to the document, as a block starting where the top-level block
    /// being read started.
    fn add(&mut self, body: Body) {
        match self.open.last_mut() {
            None => self.blocks.push((self.start, body)),
            Some(Open::Quote(blocks) | Open::Item(blocks)) => blocks.push(body),
            Some(Open::List { items, .. }) => items.push(vec![body]),
            Some(Open::Table { row, .. }) => {
                // Only a cell's text is read into a table: anything else read there goes into a cell of its own.
                let span = Span::plain(&plain_text(&body));
                row.push(Inline { spans: vec![span], ..Inline::default() });
            }
        }
    }

    /// The top-level blocks read, with every container still open closed: an unfinished reply mid-stream, an open
    /// code fence, or half a table, closes where it stops.
    fn finish(mut self) -> Vec<(usize, Body)> {
        self.flat = 0;
        self.close_leaf();
        while !self.open.is_empty() {
            self.close_container();
        }

        self.blocks
    }
}

/// The spans of a text read, an image alone among them kept as its alt text.
fn inline_of(finished: Finished) -> Inline {
    match finished {
        Finished::Inline(inline) => inline,
        Finished::Image { alt, .. } => {
            let span = Span { look: Look::Image, ..Span::plain(&alt) };
            Inline { spans: vec![span], ..Inline::default() }
        }
    }
}

/// The text of a leaf block, for a table that holds one where a cell's text is read.
fn plain_text(body: &Body) -> String {
    match body {
        Body::Paragraph(inline) | Body::Heading { text: inline, .. } => {
            inline.spans.iter().map(|span| span.text.as_str()).collect()
        }
        Body::Code { code, .. } => code.clone(),
        Body::Image { alt, .. } => alt.clone(),
        Body::Rule | Body::List { .. } | Body::Quote(_) | Body::Table { .. } => String::new(),
    }
}
