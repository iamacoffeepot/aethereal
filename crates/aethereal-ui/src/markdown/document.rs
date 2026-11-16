//! A document as it grows (ADR-0056: Markdown): a reply streams in a few tokens at a time, so the markdown read again
//! after each is the markdown before it with a little more at its end. Only the last top-level block can take in what
//! was added, a paragraph turned to a heading by the line under it, a list given another item, a fence closed, so
//! each block before it is kept as it was, the same shared value, and only the text from the start of the last block's
//! line on is read again.
//!
//! ```text
//! push_str ──> source + more ──> read from the last block's line ──> blocks before it kept, the rest replaced
//!          └─> a definition of a link by reference anywhere ──> read whole, as a block before it may use it
//! ```

use super::parse::{self, Read};
use super::{Block, Blocks, Body};
use aethereal_data::{Bare, Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use core::mem;
use std::sync::Arc;

/// A markdown document, read into its top-level blocks, which reads again only what text added at its end can change.
#[derive(Debug, Clone, Default)]
pub struct Document {
    source: String,
    blocks: Blocks,
    /// The byte each block starts at in `source`, in order.
    starts: Vec<usize>,
    reading: Reading,
}

/// How a document reads text added at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Reading {
    /// From the start of its last block's line: no block defines a link by reference.
    #[default]
    FromLast,
    /// Whole: a block defines a link by reference, which a block before it may use.
    Whole,
}

impl Document {
    /// `source`, read.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut document = Self::default();
        document.push_str(source);

        document
    }

    /// Its markdown.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Its top-level blocks, in order, each keyed by its place: shared, so a view takes them at the cost of a count.
    #[must_use]
    pub fn blocks(&self) -> Blocks {
        self.blocks.clone()
    }

    /// The sources of its images on lines of their own, in order, each once: what a caller fetches and decodes, then
    /// hands back as [`Images`](super::Images). Walked with a stack.
    #[must_use]
    pub fn images(&self) -> Vec<String> {
        let mut sources: Vec<String> = Vec::new();
        let mut stack: Vec<&Body> = self.blocks.iter().rev().map(|block| &*block.body).collect();
        while let Some(body) = stack.pop() {
            match body {
                Body::Image { source, .. } if !sources.contains(source) => sources.push(source.clone()),
                Body::Quote(blocks) => stack.extend(blocks.iter().rev()),
                Body::List { items, .. } => stack.extend(items.iter().rev().flat_map(|item| item.iter().rev())),
                Body::Image { .. }
                | Body::Paragraph(_)
                | Body::Heading { .. }
                | Body::Code { .. }
                | Body::Table { .. }
                | Body::Rule => {}
            }
        }

        sources
    }

    /// Sets its markdown to `source`: what extends its markdown is added to it as [`Self::push_str`] adds it, and
    /// anything else is read whole.
    pub fn set(&mut self, source: &str) {
        match source.strip_prefix(self.source.as_str()) {
            Some(more) => self.push_str(more),
            None => *self = Self::new(source),
        }
    }

    /// Adds `more` at its end, reading again from the start of the line its last block starts on.
    pub fn push_str(&mut self, more: &str) {
        if more.is_empty() && !self.source.is_empty() {
            return;
        }
        let from = match self.reading {
            Reading::FromLast => self.joined(),
            Reading::Whole => 0,
        };
        let mut source = mem::take(&mut self.source);
        source.push_str(more);
        self.reread(&source, from);
        self.source = source;
    }

    /// Where the last blocks that text added at its end may change start: the start of the line of its last block, or
    /// of the block before it while it may join that one, as a line added can join a block to the one before it, a
    /// line under a paragraph making it a table, backticks later on a fence's line making it a paragraph's, or a list's
    /// item after a blank line.
    fn joined(&self) -> usize {
        let Some(mut index) = self.starts.len().checked_sub(1) else {
            return 0;
        };
        loop {
            let line = line_start(&self.source, self.starts[index]);

            if index == 0 {
                return line;
            }

            // A block a blank line parts from the one before it never joins it, whatever is added after, unless that is
            // a list, whose items a blank line does not end, or code, whose indented lines a blank line does not end.
            let spans_blanks = matches!(*self.blocks[index - 1].body, Body::List { .. } | Body::Code { .. });
            let parted = parted(&self.source, line);
            if parted && !spans_blanks {
                return line;
            }
            index -= 1;
        }
    }

    /// Reads `source` again from the byte `from`, the start of a line before which every block is kept.
    fn reread(&mut self, source: &str, from: usize) {
        let Read { blocks: read, defines } = parse::read(&source[from..]);
        let kept = self.starts.partition_point(|start| *start < from);
        if defines {
            self.reading = Reading::Whole;
        }
        let rereads_whole = defines && from > 0;

        // A definition read in the tail may be used by a block before it, which only a whole reading resolves.
        if rereads_whole {
            self.starts.clear();
            self.blocks = Blocks::default();
            self.reread(source, 0);
            return;
        }
        let old = self.blocks.0.as_slice();
        let mut blocks: Vec<Block> = old[..kept.min(old.len())].to_vec();
        let mut starts: Vec<usize> = self.starts[..kept].to_vec();
        for (start, body) in read {
            let index = u32::try_from(blocks.len()).unwrap_or(u32::MAX);
            let was = old.get(blocks.len()).filter(|was| *was.body == body);
            let block = was.cloned().unwrap_or_else(|| Block { index, body: Arc::new(body) });
            blocks.push(block);
            starts.push(from + start);
        }
        self.blocks = Blocks(Arc::new(blocks));
        self.starts = starts;
    }
}

/// A document is written as its markdown, and read back by reading it, so a screen that holds one keeps it across a
/// move onto new code (ADR-0054).
impl Schema for Document {
    const SHAPE: Shape = <String as Schema>::SHAPE;
}

impl Plain for Document where String: Encode<Bare> + Decode<Bare> {}

impl<B: Boundary> Encode<B> for Document
where
    String: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.source.encode(out)
    }
}

impl<B: Boundary> Decode<B> for Document
where
    String: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        String::decode(input).map(|source| Self::new(&source))
    }
}

/// The start of the line `at` stands on in `source`: a block's indent is part of what it is, as four spaces make code.
fn line_start(source: &str, at: usize) -> usize {
    let at = at.min(source.len());

    source.as_bytes()[..at].iter().rposition(|byte| *byte == b'\n').map_or(0, |newline| newline + 1)
}

/// Whether the line before the one starting at `line` in `source` is blank.
fn parted(source: &str, line: usize) -> bool {
    let before = &source[..line.saturating_sub(1).min(source.len())];
    let previous = before.rfind('\n').map_or(before, |newline| &before[newline + 1..]);

    previous.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Body;

    #[test]
    fn a_reply_streamed_a_byte_at_a_time_reads_as_the_whole_reply_and_keeps_its_finished_blocks() {
        // Catches a tail read again from the wrong place, which splits or merges a block the whole reading does not,
        // and a block before the last one remade though nothing in it changed, which memo then builds again.
        let reply = "# Title\n\nSome *text* here.\n\n    indented code\n\n- one\n- two\n\n  more\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nTitle\n===\n\n> quote\nlazy\n";
        let mut document = Document::default();
        let mut cut = 0;
        while cut < reply.len() {
            let next = (cut + 1..=reply.len()).find(|at| reply.is_char_boundary(*at)).unwrap_or(reply.len());
            let before = document.blocks();
            document.push_str(&reply[cut..next]);
            cut = next;

            let whole = Document::new(&reply[..cut]);
            assert_eq!(document.blocks(), whole.blocks(), "after {:?}", &reply[..cut]);
            let finished = before.len().saturating_sub(1).min(document.blocks().len().saturating_sub(1));
            for (was, now) in before.iter().zip(document.blocks().iter()).take(finished) {
                assert!(Arc::ptr_eq(&was.body, &now.body), "block {} was remade after {:?}", was.index, &reply[..cut]);
            }
        }
    }

    #[test]
    fn a_link_defined_after_its_use_is_resolved_as_the_definition_streams_in() {
        // Catches a tail read on its own, which cannot see that a definition it holds names a link a block before it
        // uses.
        let mut document = Document::new("See [the docs][d].\n\n");
        document.push_str("[d]: https://example.com\n");

        let Body::Paragraph(inline) = &*document.blocks()[0].body else {
            panic!("the first block is a paragraph: {document:?}");
        };
        assert_eq!(inline.links, vec![String::from("https://example.com")]);
    }
}
