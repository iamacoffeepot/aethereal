//! What `#[protocol(…)]` lists: kinds, each told, `Kind`, or asked and answered, `Kind(answers = A)`.
//!
//! A kind is read up to the comma or the parentheses that follow it, outside its own angle brackets, as a type would
//! read `Kind(…)` as the arguments of a function type.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::parse::{Parse, ParseStream};
use syn::token::Paren;
use syn::{Ident, Token, Type, parenthesized};

/// The kinds a protocol lists, in order.
pub struct Listed {
    pub carried: Vec<Carried>,
}

/// One kind of a protocol, and what it is answered with.
pub struct Carried {
    pub kind: Type,
    pub answers: Answers,
}

/// What a kind of a protocol is answered with.
pub enum Answers {
    /// Nothing: it is told.
    Told,
    /// This kind: it is asked.
    Asked(Box<Type>),
}

impl Parse for Listed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut carried = Vec::new();
        while !input.is_empty() {
            let kind = kind(input)?;
            let answers = if input.peek(Paren) {
                answers(input)?
            } else {
                Answers::Told
            };
            carried.push(Carried { kind, answers });
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Self { carried })
    }
}

/// The kind at the head of `input`: its tokens up to a comma or a group in parentheses outside angle brackets.
fn kind(input: ParseStream<'_>) -> syn::Result<Type> {
    let tokens = input.step(|cursor| {
        let mut rest = *cursor;
        let mut tokens = TokenStream::new();
        let mut depth = 0_usize;
        while let Some((tree, next)) = rest.token_tree() {
            let ends = depth == 0 && ends_kind(&tree);
            if ends {
                break;
            }
            depth = deeper(depth, &tree);
            tokens.extend([tree]);
            rest = next;
        }

        Ok((tokens, rest))
    })?;
    if tokens.is_empty() {
        return Err(input.error("a protocol lists kinds: `Kind`, or `Kind(answers = A)` for one that is asked"));
    }

    syn::parse2(tokens)
}

/// Whether `tree`, outside angle brackets, ends a kind: a comma, or the parentheses of what it answers.
fn ends_kind(tree: &TokenTree) -> bool {
    match tree {
        TokenTree::Punct(punct) => punct.as_char() == ',',
        TokenTree::Group(group) => group.delimiter() == Delimiter::Parenthesis,
        TokenTree::Ident(_) | TokenTree::Literal(_) => false,
    }
}

/// How deep in angle brackets the tokens after `tree` stand, from `depth` before it.
fn deeper(depth: usize, tree: &TokenTree) -> usize {
    match tree {
        TokenTree::Punct(punct) if punct.as_char() == '<' => depth + 1,
        TokenTree::Punct(punct) if punct.as_char() == '>' => depth.saturating_sub(1),
        _ => depth,
    }
}

/// `(answers = A)`, after a kind that is asked.
fn answers(input: ParseStream<'_>) -> syn::Result<Answers> {
    let content;
    parenthesized!(content in input);
    let key: Ident = content.parse()?;
    if key != "answers" {
        return Err(syn::Error::new_spanned(key, "a kind of a protocol takes `answers = A`, what it is answered with"));
    }
    content.parse::<Token![=]>()?;
    let answer: Type = content.parse()?;

    Ok(Answers::Asked(Box::new(answer)))
}
