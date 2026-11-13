//! A reference (ADR-0056: References are a small subset of Rust): a path, then fields and method calls, whose
//! arguments are paths and fields alone. No operators, literals inside calls, closures, or blocks: a view that needs a
//! computed value calls a `&self` method that computes it.
//!
//! ```text
//! self.gold          self, then a field
//! item.name          a local made by `as`, then a field
//! self.stacks(item.id)    a method call whose argument is a local's field
//! Side::Below        any other path: a Rust item, which rustc resolves
//! ```

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, Index, Lit, LitInt, Member, Path, Token, parenthesized, token};

/// What every refusal of a reference says.
pub const SUBSET: &str = "a reference is a path, a field, or a method call whose arguments are paths: no operators, \
                          literals inside calls, closures, or blocks; a computed value is a `&self` method that \
                          computes it";

/// A reference read in markup.
pub struct Reference {
    pub head: Head,
    pub steps: Vec<Step>,
}

/// Where a reference starts.
pub enum Head {
    /// The screen: `self`.
    This(Token![self]),
    /// One name: a local made by `as`, a composite's param, or an item in scope.
    Name(Ident),
    /// A path of more than one segment, or from the crate's root: a Rust item.
    Path(Path),
}

/// One step after a reference's head.
pub enum Step {
    Field(Token![.], Member),
    Call { dot: Token![.], method: Ident, parens: token::Paren, arguments: Punctuated<Reference, Token![,]> },
}

/// Whether a reference may call a method: a reference may, and an argument of a call may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Calls {
    Allowed,
    Refused,
}

impl Parse for Reference {
    /// A reference, its calls' arguments paths and fields alone.
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        parse(input, Calls::Allowed)
    }
}

impl Reference {
    /// An argument of a call: a path or a field, never a call of its own.
    pub fn argument(input: ParseStream<'_>) -> syn::Result<Self> {
        parse(input, Calls::Refused)
    }

    /// Where it starts, for an error about it.
    pub fn span(&self) -> Span {
        match &self.head {
            Head::This(this) => this.span,
            Head::Name(name) => name.span(),
            Head::Path(path) => path.segments.first().map_or_else(Span::call_site, |first| first.ident.span()),
        }
    }

    /// Whether its last step is a method call: a value made for the view, rather than a place in the screen.
    pub fn ends_in_call(&self) -> bool {
        matches!(self.steps.last(), Some(Step::Call { .. }))
    }

    /// The arguments of each of its calls, in order.
    pub fn arguments(&self) -> impl Iterator<Item = &Self> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::Field(..) => None,
                Step::Call { arguments, .. } => Some(arguments.iter()),
            })
            .flatten()
    }
}

/// A reference, its calls allowed as `calls` says.
fn parse(input: ParseStream<'_>, calls: Calls) -> syn::Result<Reference> {
    let head = head(input)?;
    let mut steps = Vec::new();
    while dotted(input) {
        steps.push(step(input, calls)?);
    }

    Ok(Reference { head, steps })
}

/// Its head: `self`, a name, or a path.
fn head(input: ParseStream<'_>) -> syn::Result<Head> {
    let literal = input.peek(Lit);
    if literal {
        return Err(
            input.error("a literal is not allowed inside a call: name a `&self` method that computes the value")
        );
    }
    let this = input.peek(Token![self]) && !input.peek2(Token![::]);
    if this {
        return input.parse().map(Head::This);
    }
    let named = input.peek(Ident::peek_any) || input.peek(Token![::]);
    if !named {
        return Err(input.error(SUBSET));
    }
    let path = Path::parse_mod_style(input)?;
    let single = path.leading_colon.is_none() && path.segments.len() == 1;
    let name = path.segments.first().map(|first| first.ident.clone());
    let head = match name {
        Some(name) if single => Head::Name(name),
        _ => Head::Path(path),
    };

    Ok(head)
}

/// Whether a step follows: a `.`, which is not the `..` of a rest.
fn dotted(input: ParseStream<'_>) -> bool {
    input.peek(Token![.]) && !input.peek(Token![..])
}

/// One step: a field, or a method call if `calls` allows it.
fn step(input: ParseStream<'_>, calls: Calls) -> syn::Result<Step> {
    let dot: Token![.] = input.parse()?;
    let unnamed = input.peek(LitInt);
    if unnamed {
        let index: LitInt = input.parse()?;
        let member = Member::Unnamed(Index { index: index.base10_parse()?, span: index.span() });
        return Ok(Step::Field(dot, member));
    }
    let name: Ident = input.parse()?;
    let called = input.peek(token::Paren);
    if !called {
        return Ok(Step::Field(dot, Member::Named(name)));
    }
    if calls == Calls::Refused {
        return Err(syn::Error::new(
            name.span(),
            "an argument of a call is a path or a field, never a call of its own",
        ));
    }
    let content;
    let parens = parenthesized!(content in input);
    let arguments = Punctuated::parse_terminated_with(&content, |input| parse(input, Calls::Refused))?;

    Ok(Step::Call { dot, method: name, parens, arguments })
}

impl ToTokens for Reference {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.head {
            Head::This(this) => this.to_tokens(tokens),
            Head::Name(name) => tokens.append(name.clone()),
            Head::Path(path) => path.to_tokens(tokens),
        }
        for step in &self.steps {
            match step {
                Step::Field(dot, member) => {
                    dot.to_tokens(tokens);
                    member.to_tokens(tokens);
                }
                Step::Call { dot, method, parens, arguments } => {
                    dot.to_tokens(tokens);
                    tokens.append(method.clone());
                    parens.surround(tokens, |inner| arguments.to_tokens(inner));
                }
            }
        }
    }
}
