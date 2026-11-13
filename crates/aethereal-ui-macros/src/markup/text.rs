//! A widget's text (ADR-0056: References are a small subset of Rust): a string literal that interpolates as a
//! `format!` string does, each hole in braces held to the reference subset, `{{` and `}}` writing a brace.
//!
//! ```text
//! "Gold: {self.gold}" ──> format!("Gold: {}", self.gold)
//! "Equip"             ──> String::from("Equip")
//! ```
//!
//! Stable Rust cannot point inside a literal, so an error in a hole marks the whole string.

use super::reference::Reference;
use crate::spans;
use core::mem;
use proc_macro2::{Group, TokenStream, TokenTree};
use quote::quote_spanned;
use syn::LitStr;
use syn::parse::{ParseStream, Parser};

/// A text, read from its literal.
pub struct Text {
    pub literal: LitStr,
    pieces: Vec<Piece>,
}

/// One piece of a text.
enum Piece {
    /// Written as it stands, its braces still doubled as the format string writes them.
    Written(String),
    /// A reference, and the format spec after its `:`, empty for none.
    Hole { reference: Reference, spec: String },
}

impl Text {
    /// The text of `literal`.
    ///
    /// # Errors
    ///
    /// A brace that opens no hole or closes none, or a hole outside the reference subset, at the literal.
    pub fn parse(literal: LitStr) -> syn::Result<Self> {
        let value = literal.value();
        let mut pieces = Vec::new();
        let mut written = String::new();
        let mut chars = value.chars().peekable();
        while let Some(next) = chars.next() {
            let doubled = chars.peek() == Some(&next);
            match next {
                '{' | '}' if doubled => {
                    chars.next();
                    written.push(next);
                    written.push(next);
                }
                '{' => {
                    let hole = closed(&mut chars, &literal)?;
                    pieces.push(Piece::Written(mem::take(&mut written)));
                    pieces.push(hole_of(&hole, &literal)?);
                }
                '}' => return Err(syn::Error::new(literal.span(), "a `}` closes no hole: write `}}` for a brace")),
                _ => written.push(next),
            }
        }
        pieces.push(Piece::Written(written));

        Ok(Self { literal, pieces })
    }

    /// The references its holes read.
    pub fn holes(&self) -> impl Iterator<Item = &Reference> {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Written(_) => None,
            Piece::Hole { reference, .. } => Some(reference),
        })
    }

    /// The `String` it makes as the view builds: the literal itself when it has no holes.
    pub fn string(&self) -> TokenStream {
        let span = spans::at(self.literal.span());
        let mut format = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Written(written) => format.push_str(written),
                Piece::Hole { spec, .. } if spec.is_empty() => format.push_str("{}"),
                Piece::Hole { spec, .. } => {
                    format.push_str("{:");
                    format.push_str(spec);
                    format.push('}');
                }
            }
        }
        let holes: Vec<&Reference> = self.holes().collect();
        if holes.is_empty() {
            let unescaped = LitStr::new(&format.replace("{{", "{").replace("}}", "}"), span);
            return quote_spanned!(span=> ::std::string::String::from(#unescaped));
        }
        let format = LitStr::new(&format, span);

        quote_spanned!(span=> ::std::format!(#format, #(#holes),*))
    }
}

/// The text of a hole whose `{` was just read from `chars`, up to the `}` that closes it.
fn closed(chars: &mut impl Iterator<Item = char>, literal: &LitStr) -> syn::Result<String> {
    let mut hole = String::new();
    for next in chars {
        if next == '}' {
            return Ok(hole);
        }
        hole.push(next);
    }

    Err(syn::Error::new(literal.span(), "a `{` opens a hole no `}` closes: write `{{` for a brace"))
}

/// The hole whose text, between its braces, is `hole`, in the text `literal`.
fn hole_of(hole: &str, literal: &LitStr) -> syn::Result<Piece> {
    let (reference, spec) = split(hole);
    let tokens: TokenStream = syn::parse_str(reference).map_err(|_| unreadable(literal))?;
    let reference = (|input: ParseStream<'_>| {
        let reference: Reference = input.parse()?;
        if input.is_empty() {
            Ok(reference)
        } else {
            Err(input.error(super::reference::SUBSET))
        }
    })
    .parse2(respanned(tokens, literal))?;

    Ok(Piece::Hole { reference, spec: String::from(spec) })
}

/// A hole's reference and its format spec, split at its first `:` that is not half of a path's `::`.
fn split(hole: &str) -> (&str, &str) {
    let bytes = hole.as_bytes();
    let colon = (0..bytes.len()).find(|&at| {
        let here = bytes[at] == b':';
        let before = at > 0 && bytes[at - 1] == b':';
        let after = bytes.get(at + 1) == Some(&b':');

        here && !before && !after
    });
    colon.map_or((hole, ""), |at| (&hole[..at], &hole[at + 1..]))
}

/// `tokens`, each placed at `literal`: an error in a hole marks the whole string.
fn respanned(tokens: TokenStream, literal: &LitStr) -> TokenStream {
    let span = literal.span();
    tokens
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Group(group) => {
                let mut placed = Group::new(group.delimiter(), respanned(group.stream(), literal));
                placed.set_span(span);
                TokenTree::Group(placed)
            }
            mut other => {
                other.set_span(span);
                other
            }
        })
        .collect()
}

/// The error of a hole that is not Rust tokens at all.
fn unreadable(literal: &LitStr) -> syn::Error {
    syn::Error::new(literal.span(), "a hole in a text holds a reference: `{self.gold}`")
}
