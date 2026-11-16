//! What a token costs a screen showing a reply as it streams (ADR-0056: Markdown): a reply of about 2,000 words, laid
//! out, then grown a token at a time, its blocks each kept by `memo`, or built whole every token for comparison. It
//! measures rather than checks, so it is run by hand:
//! `cargo test --release -p aethereal-ui --lib -- --ignored a_token_costs --nocapture`.

use super::input::tests::laid;
use super::tests::screen;
use super::{Screen, Viewport};
use crate::Look;
use crate::input::Raises;
use crate::markdown::{Document, Images, Inks, Markdown, MarkdownBlock, Prose};
use crate::tree::{Element, ItemKey, Node, Segment, Tree};
use crate::widgets::Column;
use aethereal_render::font::Font as FontFile;
use aethereal_render::text::Font;
use core::time::Duration;
use std::time::Instant;

/// The renderer's own fonts.
const SANS: &[u8] = include_bytes!("../../../aethereal-render/fonts/IBMPlexSans-Regular.ttf");
const MONO: &[u8] = include_bytes!("../../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// How many tokens are added and timed.
const TOKENS: u32 = 200;

/// The tests' screen event: a link pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Open,
}

/// A reply of about 2,000 words: sections of a heading, paragraphs with marks and a link, a list, code, and a table.
fn reply() -> String {
    let sentence =
        "The quick *brown* fox jumps over the **lazy** dog and `reads` the [manual](https://example.com) twice. ";
    let mut reply = String::new();
    for section in 0..12 {
        reply.push_str("## Section ");
        reply.push_str(&section.to_string());
        reply.push_str("\n\n");
        for _ in 0..3 {
            reply.push_str(&sentence.repeat(4));
            reply.push_str("\n\n");
        }
        reply.push_str("- one item of a list\n- another item\n    - nested beneath it\n\n");
        reply.push_str("```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n");
        reply.push_str("| name | size |\n|---|---|\n| a | 1 |\n| b | 2 |\n\n");
    }
    reply.push_str("And the last paragraph, which grows");

    reply
}

/// A screen 800 by 600 that knows the renderer's fonts' metrics, whose program is made.
fn ready() -> Screen {
    let mut screen = screen();
    for (font, bytes) in [(Font::Sans, SANS), (Font::Mono, MONO)] {
        let Ok(file) = FontFile::read(bytes) else {
            panic!("the renderer's font reads");
        };
        screen.metrics_read(font, file.metrics().clone(), Vec::new());
    }
    let _ = screen.resize(Viewport::window(800, 600, 1.0));

    screen
}

/// The view of `document`: a column of its blocks, each kept by `memo` while it stands, as a screen's markup writes
/// `MarkdownBlock { memo, block: block, prose: …, link => … }` in a column of the document's blocks.
fn kept(screen: &Screen, document: &Document, prose: &Prose) -> Tree {
    let all = document.blocks();
    let blocks = all.iter().map(|block| {
        let path = [Segment::Place(0), Segment::Item(ItemKey::from(block.index))];
        let built = || MarkdownBlock {
            block: block.clone(),
            prose: prose.clone(),
            images: Images::default(),
            link: Raises::Event(Ev::Open),
        };
        screen.memo(&path, Look::Own, built, |kept| kept.block == *block && kept.prose == *prose)
    });

    Tree::new(Node::new(Segment::Place(0), Column, Inks::default().document, blocks.collect()))
}

/// The view of `document` built whole.
fn whole(document: &Document, prose: &Prose) -> Tree {
    let markdown = Markdown {
        blocks: document.blocks(),
        prose: prose.clone(),
        images: Images::default(),
        link: Raises::Event(Ev::Open),
    };

    Tree::new(markdown.node(Segment::Place(0), Vec::new()))
}

/// How long each of [`TOKENS`] tokens takes on average, from the token's arrival to its draws sent, the view built by
/// `view`, and how many orders each sends.
#[allow(clippy::disallowed_methods)] // A measurement on the machine it runs on, never in a browser.
fn timed(view: impl Fn(&Screen, &Document, &Prose) -> Tree) -> (Duration, usize) {
    let prose = Prose::default();
    let mut screen = ready();
    let mut document = Document::new(&reply());
    for _ in 0..2 {
        let tree = view(&screen, &document, &prose);
        let _ = laid(&mut screen, tree, &[]);
    }
    let mut orders = 0;
    let started = Instant::now();
    for token in 0..TOKENS {
        document.push_str(&format!(" word{token}"));
        let tree = view(&screen, &document, &prose);
        orders += laid(&mut screen, tree, &[]).1.len();
    }

    (started.elapsed() / TOKENS, orders / TOKENS as usize)
}

/// How long reading one token takes on average, of [`TOKENS`] added to the reply.
#[allow(clippy::disallowed_methods)] // A measurement on the machine it runs on, never in a browser.
fn read() -> Duration {
    let mut document = Document::new(&reply());
    let started = Instant::now();
    for token in 0..TOKENS {
        document.push_str(&format!(" word{token}"));
    }

    started.elapsed() / TOKENS
}

#[test]
#[ignore = "a measurement, run by hand"]
#[allow(clippy::print_stdout)] // It reports what it measured.
fn a_token_costs_the_block_it_grows() {
    let words = reply().split_whitespace().count();
    let (memo, memo_orders) = timed(kept);
    let (rebuilt, rebuilt_orders) = timed(|_, document, prose| whole(document, prose));

    println!("words={words} tokens={TOKENS} read_per_token_micros={}", read().as_micros());
    println!("memo per_token_micros={} orders_per_token={memo_orders}", memo.as_micros());
    println!("whole per_token_micros={} orders_per_token={rebuilt_orders}", rebuilt.as_micros());
}
