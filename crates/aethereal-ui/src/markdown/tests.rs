//! Markdown read and built: the shape each construct builds, a reply that grows keeping its finished blocks in the
//! tree comparison, nesting past the cap read as text, and arbitrary markdown never panicking.

use super::{Block, Body, Document, Inks, Look, MOST_NESTING, Markdown, MarkdownBlock, Prose};
use crate::fonts::Texts;
use crate::input::Raises;
use crate::style::{Style, Track};
use crate::tree::{Element, ItemKey, KeyPath, Look as Styled, Memos, Node, Segment, Shape, Tree};
use crate::widget::Axis;
use crate::widgets::{Column, Flow, Grid, Link, Rich, Row, Scroll};
use aethereal_render::text::Font;

/// The tests' screen event: a link pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Open,
}

/// The tree of `source` built whole, styled by the default style set.
fn built(source: &str) -> Tree {
    let document = Document::new(source);
    let markdown = Markdown { blocks: document.blocks(), prose: Prose::default(), link: Raises::Event(Ev::Open) };

    Tree::new(markdown.node(Segment::Place(0), Vec::new()))
}

/// The name of a widget of the kinds markdown builds, a text's with its text.
fn name(node: &Node) -> String {
    let widget = node.widget().as_any();
    if let Some(rich) = widget.downcast_ref::<Rich<Ev>>() {
        let text: String = rich.runs.iter().map(|run| run.text.as_str()).collect();
        return format!("Rich({text})");
    }
    let names = [
        widget.is::<Column>().then_some("Column"),
        widget.is::<Row>().then_some("Row"),
        widget.is::<Grid>().then_some("Grid"),
        widget.is::<Scroll>().then_some("Scroll"),
    ];

    String::from(names.into_iter().flatten().next().unwrap_or("?"))
}

/// The shape of `tree`: each node its widget's name, followed by its children in brackets, written with a stack.
fn shape(tree: &Tree) -> String {
    let mut kept = Shape::default();
    let Ok(flattened) = tree.flat(&mut kept) else {
        panic!("the tree is walked");
    };
    let nodes = &flattened.nodes;
    let mut written = String::new();
    let mut stack = vec![Step::Open(0)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Open(at) => {
                written.push_str(&name(nodes[at].node));
                let children = nodes[at].children;
                if children.is_empty() {
                    continue;
                }
                written.push('[');
                stack.push(Step::Close);
                for (place, child) in children.iter().enumerate().rev() {
                    stack.push(Step::Open(*child));
                    if place > 0 {
                        stack.push(Step::Space);
                    }
                }
            }
            Step::Space => written.push(' '),
            Step::Close => written.push(']'),
        }
    }

    written
}

/// One step of writing a tree's shape.
enum Step {
    Open(usize),
    Space,
    Close,
}

/// The first widget of `tree` in tree order that is a `W`, with its style.
fn first<W: Clone + 'static>(tree: &Tree) -> (W, Style) {
    let mut kept = Shape::default();
    let Ok(flattened) = tree.flat(&mut kept) else {
        panic!("the tree is walked");
    };
    let found = flattened.nodes.iter().find_map(|flat| {
        let widget = flat.node.widget().as_any().downcast_ref::<W>()?;
        Some((widget.clone(), flat.node.style().clone()))
    });
    let Some(found) = found else {
        panic!("the tree holds the widget sought");
    };

    found
}

#[test]
fn nested_lists_build_a_row_for_each_item_of_its_marker_and_a_column_of_its_blocks() {
    // Catches an item's blocks set beside its marker rather than within a column of their own, and a nested list
    // built beside its item rather than within it.
    let tree = built("- one\n    - two\n3. three\n");

    assert_eq!(
        shape(&tree),
        "Column[Column[Row[Rich(\u{2022}) Column[Rich(one) Column[Row[Rich(\u{2022}) Column[Rich(two)]]]]]] \
         Column[Row[Rich(3.) Column[Rich(three)]]]]"
    );
}

#[test]
fn a_table_builds_a_grid_of_a_column_for_each_cell_across_its_header_first_its_short_rows_filled() {
    // Catches a grid without a column for each cell across, which lays every cell in one column, and a short row left
    // short, which shifts each cell after it into the row before.
    let tree = built("| name | size |\n|---|---|\n| a | 1 |\n| b |\n");

    assert_eq!(shape(&tree), "Column[Grid[Rich(name) Rich(size) Rich(a) Rich(1) Rich(b) Rich()]]");
    let (_, grid) = first::<Grid>(&tree);
    assert_eq!(grid.columns, vec![Track::Auto, Track::Auto]);
}

#[test]
fn a_code_block_is_its_code_unbroken_in_monospace_scrolled_across_on_a_panel_its_language_kept() {
    // Catches code wrapped at the column's width, set in the proportional font, or left with no scroll across, and a
    // fence's language lost.
    let source = "```rust title\nfn main() {}\n```\n";
    let tree = built(source);

    assert_eq!(shape(&tree), "Column[Column[Scroll[Rich(fn main() {})]]]");
    let (code, styled) = first::<Rich<Ev>>(&tree);
    assert_eq!(code.flow, Flow::Unbroken);
    assert_eq!(styled.font, Font::Mono);
    let (scroll, _) = first::<Scroll>(&tree);
    assert_eq!(scroll.axis, Axis::Row);
    let document = Document::new(source);
    assert!(matches!(&*document.blocks()[0].body, Body::Code { language, .. } if language == "rust"), "{document:?}");
}

#[test]
fn marks_and_links_in_a_paragraph_are_runs_of_one_text_each_in_its_colour() {
    // Catches a span built as a widget of its own, a mark drawn in the plain colour, and a link that names the wrong
    // run or target.
    let tree = built("plain *em* **strong** ~~gone~~ `code` [link](https://example.com)");
    let inks = Inks::default();

    let (rich, _) = first::<Rich<Ev>>(&tree);
    let runs: Vec<(&str, _)> = rich.runs.iter().map(|run| (run.text.as_str(), run.colour)).collect();
    let plain = inks.paragraph.colour;
    assert_eq!(
        runs,
        vec![
            ("plain ", plain),
            ("em", inks.emphasis),
            (" ", plain),
            ("strong", inks.strong),
            (" ", plain),
            ("gone", inks.strike),
            (" ", plain),
            ("code", inks.inline_code),
            (" ", plain),
            ("link", inks.link),
        ]
    );
    assert_eq!(rich.links, vec![Link { run: 9, target: String::from("https://example.com") }]);
}

#[test]
fn headings_quotes_rules_and_images_build_their_own_boxes_and_deep_headings_are_styled_as_the_third() {
    // Catches a heading past the third level styled out of the set's range or as a paragraph, a quote without its bar,
    // and an image on its own line lost rather than shown by its alt text.
    let tree = built("# One\n\n#### Four\n\n> quoted\n\n---\n\n![a cat](cat.png)\n");
    let inks = Inks::default();

    assert_eq!(shape(&tree), "Column[Rich(One) Rich(Four) Row[Column Column[Rich(quoted)]] Column Rich(a cat)]");
    let mut kept = Shape::default();
    let Ok(flattened) = tree.flat(&mut kept) else {
        panic!("the tree is walked");
    };
    assert_eq!(flattened.nodes[1].node.style().font_size, inks.headings[0].font_size);
    assert_eq!(flattened.nodes[2].node.style().font_size, inks.headings[2].font_size);
    let document = Document::new("![a cat](cat.png)");
    assert_eq!(*document.blocks()[0].body, Body::Image { alt: String::from("a cat"), source: String::from("cat.png") });
}

/// How many containers deep `body` nests, walked with a stack.
fn nesting(body: &Body) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(body, 0)];
    while let Some((body, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        match body {
            Body::Quote(blocks) => stack.extend(blocks.iter().map(|block| (block, depth + 1))),
            Body::List { items, .. } => {
                stack.extend(items.iter().flatten().map(|block| (block, depth + 2)));
            }
            Body::Paragraph(_)
            | Body::Heading { .. }
            | Body::Code { .. }
            | Body::Table { .. }
            | Body::Rule
            | Body::Image { .. } => {}
        }
    }

    deepest
}

/// Every text of `body`'s paragraphs, walked with a stack.
fn texts(body: &Body) -> String {
    let mut written = String::new();
    let mut stack = vec![body];
    while let Some(body) = stack.pop() {
        match body {
            Body::Paragraph(inline) => written.extend(inline.spans.iter().map(|span| span.text.as_str())),
            Body::Quote(blocks) => stack.extend(blocks.iter().rev()),
            Body::List { items, .. } => stack.extend(items.iter().flatten().rev()),
            Body::Heading { .. } | Body::Code { .. } | Body::Table { .. } | Body::Rule | Body::Image { .. } => {}
        }
    }

    written
}

#[test]
fn containers_nested_past_the_cap_are_read_as_plain_text_and_the_tree_stays_within_its_depth() {
    // Catches untrusted nesting followed all the way down, which overflows the screen's tree, and the deep part
    // dropped rather than shown as text.
    let quotes = format!("{} deep words", "> ".repeat(200));
    let lists: Vec<String> = (0..200).map(|depth| format!("{}- item {depth}\n", "  ".repeat(depth))).collect();
    let lists = lists.concat();

    for source in [quotes.as_str(), lists.as_str()] {
        let document = Document::new(source);
        let body = &document.blocks()[0].body;
        assert!(nesting(body) <= MOST_NESTING, "nested {} deep", nesting(body));
        let tree = built(source);
        let mut kept = Shape::default();
        assert!(tree.flat(&mut kept).is_ok(), "the tree stands within its depth");
        let read = texts(body);
        assert!(read.contains("deep words") || read.contains("item 199"), "the deep part is shown: {read:?}");
    }
}

/// A deterministic run of numbers, the same every time, from `seed`.
struct Numbers {
    state: u64,
}

impl Numbers {
    /// The next number below `below`.
    fn below(&mut self, below: usize) -> usize {
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let high = usize::try_from(self.state >> 33).unwrap_or(0);

        high % below.max(1)
    }
}

/// The pieces arbitrary markdown is made of: every construct's marks, broken and unbalanced, and text that is not ASCII.
const PIECES: &[&str] = &[
    "*",
    "**",
    "_",
    "__",
    "`",
    "```",
    "~~~",
    "~~",
    "[",
    "]",
    "](",
    ")",
    "![",
    "|",
    "|---|",
    "---",
    "\n",
    "\n\n",
    "> ",
    "- ",
    "* ",
    "1. ",
    "2) ",
    "# ",
    "###### ",
    "    ",
    "\t",
    "<b>",
    "</b>",
    "<!--",
    "&amp;",
    "\\",
    "é",
    "\u{1f600}",
    "[x]: http://x",
    "[x]",
    "word",
    " ",
    "http://a.b",
    "<http://c>",
    "\r\n",
    "\u{0}",
    "=",
    "+ ",
];

#[test]
fn arbitrary_markdown_is_read_built_and_streamed_without_panicking() {
    // Catches a panic on Claude's untrusted output anywhere in reading, building, or streaming, and a reply streamed in
    // pieces that reads otherwise than the same reply read whole.
    let mut numbers = Numbers { state: 0x5eed };
    for _ in 0..300 {
        let count = 1 + numbers.below(60);
        let source: String = (0..count).map(|_| PIECES[numbers.below(PIECES.len())]).collect();

        let tree = built(&source);
        let mut kept = Shape::default();
        assert!(tree.flat(&mut kept).is_ok(), "{source:?}");
        let mut streamed = Document::default();
        let mut at = 0;
        while at < source.len() {
            let step = 1 + numbers.below(7);
            let next = (at + step..=source.len()).find(|end| source.is_char_boundary(*end)).unwrap_or(source.len());
            streamed.push_str(&source[at..next]);
            at = next;
        }
        assert_eq!(streamed.blocks(), Document::new(&source).blocks(), "{source:?}");
    }
}

/// The blocks of `document` built for a screen, each at its key, as a screen's view builds them in a column.
fn transcript(document: &Document) -> Tree {
    let prose = Prose::default();
    let all = document.blocks();
    let blocks = all.iter().map(|block| {
        let built = MarkdownBlock { block: block.clone(), prose: prose.clone(), link: Raises::Event(Ev::Open) };
        built.node(Segment::Item(block.index.into()), Vec::new())
    });

    Tree::new(Node::new(Segment::Place(0), Column, Inks::default().document, blocks.collect()))
}

#[test]
fn a_reply_that_grows_keeps_its_finished_blocks_unchanged_in_the_tree_comparison() {
    // Catches a block before the last remade as a reply streams, which the comparison then lays out and draws again:
    // the cost of each token would be the whole reply's.
    let mut document = Document::new("# Title\n\nA first paragraph.\n\n- a list\n\nThe last");
    let mut memos = Memos::default();
    let mut texts = Texts::default();
    let mut last = transcript(&document);
    let _ = last.first(&mut memos, 1);

    document.push_str(" paragraph grows");
    let mut grown = transcript(&document);
    let _ = last.merge(&mut grown, &mut texts, &mut memos, 2);

    let path = |index: u64| KeyPath::of(&[Segment::Place(0), Segment::Item(ItemKey::new(index))]);
    for index in 0..3 {
        let Some(node) = grown.find(&path(index)) else {
            panic!("block {index} stands");
        };
        assert_eq!((node.stamps().laid, node.stamps().drawn), (1, 1), "block {index} is unchanged");
    }
    let Some(changed) = grown.find(&path(3)) else {
        panic!("the last block stands");
    };
    assert_eq!(changed.stamps().drawn, 2, "the grown block draws anew");
}

#[test]
fn a_block_that_stands_is_kept_by_memo_and_the_one_that_grew_is_built() {
    // Catches a block that compares unequal to itself as the reply grows, so `memo` builds every block at every token.
    let mut document = Document::new("First.\n\nSecond");
    let prose = Prose::default();
    let mut memos = Memos::default();
    let node = |memos: &Memos, block: &Block| {
        let path = [Segment::Place(0), Segment::Item(block.index.into())];
        let built = || MarkdownBlock { block: block.clone(), prose: prose.clone(), link: Raises::Event(Ev::Open) };
        memos.node(&path, Styled::Own, built, |kept| kept.block == *block && kept.prose == prose)
    };
    let mut first = Tree::new(Node::new(
        Segment::Place(0),
        Column,
        Inks::default().document,
        document.blocks().iter().map(|block| node(&memos, block)).collect(),
    ));
    let _ = first.first(&mut memos, 1);

    document.push_str(" grows.");
    let kept: Vec<bool> = document.blocks().iter().map(|block| node(&memos, block).is_kept()).collect();

    assert_eq!(kept, vec![true, false]);
}

#[test]
fn an_image_inside_a_line_of_text_shows_its_alt_text_in_the_line() {
    // Catches an inline image dropped from its line, or made a block of its own, which breaks the paragraph around it.
    let document = Document::new("before ![icon](i.png) after");

    let Body::Paragraph(inline) = &*document.blocks()[0].body else {
        panic!("one paragraph: {document:?}");
    };
    let looks: Vec<(&str, Look)> = inline.spans.iter().map(|span| (span.text.as_str(), span.look)).collect();
    assert_eq!(looks, vec![("before ", Look::Plain), ("icon", Look::Image), (" after", Look::Plain)]);
}
