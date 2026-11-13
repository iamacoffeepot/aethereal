//! Markup read from tokens (ADR-0056: Markup): `Type { settings, events, children, text }`, its settings and events
//! separated by commas and before its children and text.
//!
//! ```text
//! Grid { style: slots, items: self.items as item key item.id,     settings: name: value, keywords first-class
//!     Text { "{item.name}" }                                       a child: an element; or a text: a string literal
//! }
//! ```

use super::reference::{Reference, SUBSET};
use super::text::Text;
use super::{Call, Drag, Dropping, Element, Event, Field, Kind, Look, Outside, Repeat, Rest, Shown, Value};
use crate::style::Property;
use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, Lit, LitStr, Path, Token, braced, parenthesized, token};

impl Parse for Element {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let kind = kind(input)?;
        let content;
        let braces = braced!(content in input);
        let mut element = Self::bare(kind, braces.span.join());
        contents(&content, &mut element)?;

        Ok(element)
    }
}

/// What an element is: the keyword `child`, or a widget's type, capitalized.
fn kind(input: ParseStream<'_>) -> syn::Result<Kind> {
    let fork = input.fork();
    let keyword = fork.parse::<Ident>().is_ok_and(|name| name == "child") && fork.peek(token::Brace);
    if keyword {
        return input.parse().map(Kind::Child);
    }
    let text = input.peek(LitStr);
    if text {
        return Err(input.error("a text comes after an element's settings, within its braces"));
    }
    let path: Path = input
        .parse()
        .map_err(|error| syn::Error::new(error.span(), "an element is a widget's type and its braces: `Text { … }`"))?;
    let capital = path.segments.last().is_some_and(|last| last.ident.to_string().starts_with(char::is_uppercase));
    if !capital {
        let message = "an element is named by its widget's type, which is capitalized; the one lowercase element is \
                       `child`";
        return Err(syn::Error::new_spanned(&path, message));
    }
    let braced = input.peek(token::Brace);
    if !braced {
        return Err(input.error("an element's settings, children, and text are in its braces: `Text { … }`"));
    }

    Ok(Kind::Widget(path))
}

/// Where in an element's braces the next item stands: its settings come first, then its children and text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Settings,
    Content,
}

/// The contents of `element`'s braces, read into it.
fn contents(input: ParseStream<'_>, element: &mut Element) -> syn::Result<()> {
    let mut stage = Stage::Settings;
    while !input.is_empty() {
        let rest = input.peek(Token![..]);
        let named = input.peek(Ident) && !input.peek2(Token![::]);
        let setting = named && input.peek2(Token![:]);
        let event = named && input.peek2(Token![=>]);
        let text = input.peek(LitStr);
        if rest {
            settled(stage, input.span())?;
            let dots: Token![..] = input.parse()?;
            element.rest = Rest::Defaulted(dots.spans[0]);
            separated(input)?;
        } else if setting {
            settled(stage, input.span())?;
            let name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            set(input, name, element)?;
            separated(input)?;
        } else if event {
            settled(stage, input.span())?;
            let name: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            raise(input, name, element)?;
            separated(input)?;
        } else if text {
            stage = Stage::Content;
            let literal: LitStr = input.parse()?;
            let span = literal.span();
            let twice = element.text.replace(Text::parse(literal)?).is_some();
            if twice {
                return Err(syn::Error::new(span, "an element has one text"));
            }
            input.parse::<Option<Token![,]>>()?;
        } else {
            stage = Stage::Content;
            element.children.push(input.parse()?);
            input.parse::<Option<Token![,]>>()?;
        }
    }

    Ok(())
}

/// Refuses a setting, an event, or a rest after an element's children or text.
fn settled(stage: Stage, span: Span) -> syn::Result<()> {
    match stage {
        Stage::Settings => Ok(()),
        Stage::Content => Err(syn::Error::new(span, "settings and events come before an element's children and text")),
    }
}

/// The comma after a setting or an event, unless it ends the braces.
fn separated(input: ParseStream<'_>) -> syn::Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    let comma = input.peek(Token![,]);
    if !comma {
        let message = format!("a setting or an event ends at a comma; {SUBSET}");
        return Err(input.error(message));
    }
    input.parse::<Token![,]>()?;

    Ok(())
}

/// The setting `name`'s value, read into `element`: one of the keywords, or a field of its widget.
fn set(input: ParseStream<'_>, name: Ident, element: &mut Element) -> syn::Result<()> {
    let twice = || syn::Error::new(name.span(), format!("`{name}` is given twice"));
    match name.to_string().as_str() {
        "style" => {
            let unset = matches!(element.look, Look::Unstyled);
            if !unset {
                return Err(twice());
            }
            element.look = look(input)?;
        }
        "show" => {
            let unset = matches!(element.shown, Shown::Always);
            if !unset {
                return Err(twice());
            }
            element.shown = shown(input)?;
        }
        "items" => {
            let unset = matches!(element.repeat, Repeat::Once);
            if !unset {
                return Err(twice());
            }
            element.repeat = repeat(input)?;
        }
        "drag" => {
            let unset = matches!(element.drag, Drag::Fixed);
            if !unset {
                return Err(twice());
            }
            element.drag = Drag::Carries(input.parse()?);
        }
        _ => element.fields.push(Field { name, value: input.parse()? }),
    }

    Ok(())
}

/// The event `name`'s call, read into `element`: a drop, a drag's `drop_outside`, or a field of its widget.
fn raise(input: ParseStream<'_>, name: Ident, element: &mut Element) -> syn::Result<()> {
    let twice = || syn::Error::new(name.span(), format!("`{name}` is given twice"));
    match name.to_string().as_str() {
        "drop" => {
            let unset = matches!(element.drop, Dropping::Refuses);
            if !unset {
                return Err(twice());
            }
            element.drop = Dropping::Takes(input.parse()?);
        }
        "drop_outside" => {
            let unset = matches!(element.outside, Outside::Nothing);
            if !unset {
                return Err(twice());
            }
            element.outside = Outside::Raises(input.parse()?);
        }
        _ => element.events.push(Event { name, call: input.parse()? }),
    }

    Ok(())
}

impl Parse for Value {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let text = input.peek(LitStr);
        let negative = input.peek(Token![-]) && input.peek2(Lit);
        let literal = input.peek(Lit) || negative;
        if text {
            return input.parse().map(Self::Text);
        }
        if literal {
            let sign = input.parse::<Option<Token![-]>>()?;
            let literal: Lit = input.parse()?;
            return Ok(Self::Literal(quote::quote!(#sign #literal)));
        }

        input.parse().map(Self::Reference)
    }
}

impl Parse for Call {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let this = input.peek(Token![self]);
        if this {
            let this = input.parse()?;
            input.parse::<Token![.]>()?;
            let method = input.parse()?;
            let content;
            parenthesized!(content in input);
            let arguments = Punctuated::<Reference, Token![,]>::parse_terminated_with(&content, Reference::argument)?;
            return Ok(Self::Method { this, method, arguments: arguments.into_iter().collect() });
        }
        let raise = input.peek(Ident);
        if !raise {
            let message = "an event calls a method of the screen, `click => self.select(item.id)`, or in a composite \
                           passes on one of its `raises`, `click => click`";
            return Err(input.error(message));
        }

        input.parse().map(Self::Raise)
    }
}

/// A `style` setting: a style's path, extended in braces for this element alone.
fn look(input: ParseStream<'_>) -> syn::Result<Look> {
    let path = Path::parse_mod_style(input)?;
    let extended = input.peek(token::Brace);
    if !extended {
        return Ok(Look::Named(path));
    }
    let content;
    braced!(content in input);
    let properties = Punctuated::<Property, Token![,]>::parse_terminated(&content)?;

    Ok(Look::Extended(path, properties.into_iter().collect()))
}

/// A `show` setting: a `bool`, or an `Option` and the local `as` names its value.
fn shown(input: ParseStream<'_>) -> syn::Result<Shown> {
    let reference = input.parse()?;
    let bound = input.peek(Token![as]);
    if !bound {
        return Ok(Shown::While(reference));
    }
    input.parse::<Token![as]>()?;

    Ok(Shown::Bound(reference, input.parse()?))
}

/// What an `items` setting names.
const ITEMS: &str =
    "`items` names its list, the local for each item, and its key: `items: self.items as item key item.id`";

/// An `items` setting: `list as local key reference`.
fn repeat(input: ParseStream<'_>) -> syn::Result<Repeat> {
    let list = input.parse()?;
    input.parse::<Token![as]>().map_err(|error| syn::Error::new(error.span(), ITEMS))?;
    let local = input.parse()?;
    let key: Ident = input.parse()?;
    if key != "key" {
        return Err(syn::Error::new(key.span(), "`items` names the key that tells its items apart: `key item.id`"));
    }

    Ok(Repeat::Items { list, local, key: input.parse()? })
}
