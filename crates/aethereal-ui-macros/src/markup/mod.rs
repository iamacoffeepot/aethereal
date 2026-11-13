//! The markup `view!` and `widget!` share (ADR-0056: Markup): an element is a widget's type and its braces, holding
//! its settings, `name: value`, and its events, `name => call`, separated by commas, then its children and its text.
//!
//! ```text
//! tokens ──parse──> Element ──check (Names: what a reference may name in this macro)──> errors at their tokens
//!                      └──emit (Owner: a screen's view or a composite's tree)──> Node, built by the runtime's own calls
//! ```
//!
//! A lowercase name is a keyword: the element `child`, and the settings `style`, `show`, `items`, and `drag`, and the
//! events `drop` and `drop_outside`, which belong to the node rather than the widget. Every other name is a field of
//! the widget's type.

mod check;
mod emit;
mod parse;
mod reference;
mod text;

pub use check::{Names, Scope, check};
pub use emit::{Emitter, Owner, variant};
pub use reference::Head;

use crate::style::Property;
use proc_macro2::{Span, TokenStream};
use reference::Reference;
use syn::{Ident, LitStr, Path, Token};
use text::Text;

/// One element of markup: a widget, or the slot of a child actor, with what its markup gives it.
pub struct Element {
    pub kind: Kind,
    /// The whole element, for an error about it as a whole.
    pub span: Span,
    /// Its `name: value` settings that are fields of its widget, in order.
    pub fields: Vec<Field>,
    /// Its `name => call` events that are fields of its widget, in order.
    pub events: Vec<Event>,
    pub look: Look,
    pub shown: Shown,
    pub repeat: Repeat,
    pub drag: Drag,
    pub drop: Dropping,
    pub outside: Outside,
    pub rest: Rest,
    pub children: Vec<Self>,
    /// Its text, the string literal among its children: none for an element whose markup writes none.
    pub text: Option<Text>,
}

/// What an element is.
pub enum Kind {
    /// A widget, by the path of its type.
    Widget(Path),
    /// The keyword `child`: the slot of a child actor the screen holds.
    Child(Ident),
}

/// A setting that is a field of the widget: `name: value`.
pub struct Field {
    pub name: Ident,
    pub value: Value,
}

/// A setting's value: a literal, parsed by the field's type, or a reference.
pub enum Value {
    /// A string literal, turned into the field's type as it is set.
    Text(LitStr),
    /// Any other literal, a number or a flag, written as it is.
    Literal(TokenStream),
    Reference(Reference),
}

/// An event that is a field of the widget: `name => call`.
pub struct Event {
    pub name: Ident,
    pub call: Call,
}

/// What an event names.
pub enum Call {
    /// A method of the screen, and the arguments the view copies: `self.select(item.id)`.
    Method { this: Token![self], method: Ident, arguments: Vec<Reference> },
    /// One of a composite's `raises`, passed on: `click`.
    Raise(Ident),
}

/// The style an element names.
pub enum Look {
    /// None: the default style.
    Unstyled,
    /// A style function, by its path: `style: title`.
    Named(Path),
    /// A style function, extended for this element alone: `style: grid { columns: 6 }`.
    Extended(Path, Vec<Property>),
}

/// When an element is in the tree.
pub enum Shown {
    Always,
    /// While a `bool` is true: `show: self.unlocked()`.
    While(Reference),
    /// While an `Option` is `Some`, its value named by a local: `show: self.selected_item() as item`.
    Bound(Reference, Ident),
}

/// How many times an element's one child is built.
pub enum Repeat {
    /// Each child once, at its place in the markup.
    Once,
    /// Its one child once for each item of `list`, named `local`, told apart by `key`.
    Items { list: Reference, local: Ident, key: Reference },
}

/// Whether an element is dragged.
pub enum Drag {
    Fixed,
    /// It is, carrying this payload, copied as the view builds: `drag: item.id`.
    Carries(Reference),
}

/// Whether an element takes drops.
pub enum Dropping {
    Refuses,
    /// It does, raising this event with the payload: `drop => self.place(item.slot)`.
    Takes(Call),
}

/// What a drag from an element raises when nothing takes it.
pub enum Outside {
    Nothing,
    /// This event, with the payload: `drop_outside => self.discard()`.
    Raises(Call),
}

/// Whether the fields the markup leaves out take the widget's defaults.
pub enum Rest {
    /// None is left out: the markup writes every field, as a struct literal does.
    Written,
    /// `..`: the rest are its type's `Default`, as a struct literal's `..Default::default()`.
    Defaulted(Span),
}

impl Element {
    /// An element of `kind` at `span` whose markup gives it nothing yet.
    fn bare(kind: Kind, span: Span) -> Self {
        Self {
            kind,
            span,
            fields: Vec::new(),
            events: Vec::new(),
            look: Look::Unstyled,
            shown: Shown::Always,
            repeat: Repeat::Once,
            drag: Drag::Fixed,
            drop: Dropping::Refuses,
            outside: Outside::Nothing,
            rest: Rest::Written,
            children: Vec::new(),
            text: None,
        }
    }

    /// Whether it is the stock widget named `name`, by the last segment of its type's path.
    pub fn is(&self, name: &str) -> bool {
        match &self.kind {
            Kind::Widget(path) => path.segments.last().is_some_and(|last| last.ident == name),
            Kind::Child(_) => false,
        }
    }

    /// The field it sets named `name`, if it sets one.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }
}

impl Call {
    /// Where it is written.
    pub fn span(&self) -> Span {
        match self {
            Self::Method { method, .. } => method.span(),
            Self::Raise(name) => name.span(),
        }
    }
}
