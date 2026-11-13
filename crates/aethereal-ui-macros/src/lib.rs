//! The macros of the widget library (ADR-0056: Expanded by a macro, never read at runtime), re-exported by
//! `aethereal-ui`, so a screen writes `aethereal_ui::view!`:
//!
//! ```text
//! view!   { Screen, field, Root { … } } ──> the screen's event enum, one `impl View`, and `screen!(Screen, field)`
//! style!  { pub name: base { prop: value, … } … } ──> one `pub fn name() -> Style` for each style
//! widget! { struct Name { param: Type } raises ev; Root { … } } ──> the composite's type, and how it builds its tree
//! #[events] impl Screen { fn method(…) } ──> the types of each method's arguments, which the event enum holds
//! ```
//!
//! The markup `view!` and `widget!` share is in its `markup` module: each is parsed into elements, checked, and written out as
//! the runtime's own calls, each carrying the span of the markup that wrote it, so a rustc error on generated code
//! points at the markup. What a macro checks itself, it refuses with a `compile_error!` at the offending token.
//!
//! The library's docs show each in use: `aethereal_ui::view!`, `aethereal_ui::style!`, `aethereal_ui::widget!`, and
//! `aethereal_ui::events`.

mod events;
mod markup;
mod spans;
mod style;
mod view;
mod widget;

use proc_macro::TokenStream;

/// A screen's view, written as markup: its type, the field that holds its `Screen`, and the root of its tree. It
/// expands to the screen's event enum, `{Screen}Event`, one variant for each method its events call; one
/// `impl View for Screen`, whose `view` builds the tree and whose `on` calls the method each event names; and the
/// call of `screen!(Screen, field)`. An event that copies arguments names their types from the screen's methods,
/// which `#[events]` on the impl that holds them declares.
#[proc_macro]
pub fn view(input: TokenStream) -> TokenStream {
    view::expand(input.into()).into()
}

/// Styles, each a `pub fn name() -> Style`: `pub name: base { prop: value, … }` starts from `base()`, and
/// `pub name { … }` from the default style. A property is a field of `Style`; a number is its pixels, or for
/// `columns` and `rows` a count of equal tracks, and a colour is `rgb(…)`, `rgba(…)`, or `"#rrggbb"`.
#[proc_macro]
pub fn style(input: TokenStream) -> TokenStream {
    style::expand(input.into()).into()
}

/// A composite, written as markup: `struct Name { param: Type, … }`, the events it raises, `raises click, …;`, and the
/// root of its tree. It expands to the type, its params and its events as fields, generic over the screen's event type
/// when it raises any, and to how it builds its tree from them, which a screen's view calls as it would place a
/// primitive.
#[proc_macro]
pub fn widget(input: TokenStream) -> TokenStream {
    widget::expand(input.into()).into()
}

/// Declares the types of the arguments of the methods in this impl, after the turn, so a screen's `view!` names them
/// in its event enum: the macro sees only the markup, and an event names a method, not its types.
#[proc_macro_attribute]
pub fn events(attribute: TokenStream, item: TokenStream) -> TokenStream {
    events::expand(attribute.into(), item.into()).into()
}
