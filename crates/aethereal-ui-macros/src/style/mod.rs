//! `style!` (ADR-0056: Styles are Rust items): each style a function, `pub fn name() -> Style`, that starts from one
//! other style, or from the default, and sets properties over it. An element's own extension of a style,
//! `style: grid { columns: 6 }`, is written with the same properties.
//!
//! ```text
//! pub detail: panel { padding: 16 } ──> pub fn detail() -> Style { Style { padding: Edges::all(Space::Pixels(16.0)), ..panel() } }
//! ```

mod check;
mod emit;
mod parse;

pub use check::properties;
pub use emit::extended;

use proc_macro2::TokenStream;
use syn::parse::Parser;
use syn::{Attribute, Expr, Ident, Path, Visibility};

/// One style a `style!` defines.
struct Definition {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    base: Base,
    properties: Vec<Property>,
}

/// What a style starts from.
enum Base {
    /// The default style.
    Default,
    /// Another style, by its path.
    Style(Path),
}

/// One property a style sets: a field of the runtime's `Style`, and its value.
pub struct Property {
    pub name: Ident,
    pub value: Expr,
}

/// Expands `style!`'s input into one function for each style it defines.
pub fn expand(input: TokenStream) -> TokenStream {
    let expanded = parse::definitions.parse2(input).and_then(|definitions| {
        check::definitions(&definitions)?;
        Ok(emit::definitions(&definitions))
    });

    expanded.unwrap_or_else(syn::Error::into_compile_error)
}
