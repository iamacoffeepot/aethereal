//! `widget!` (ADR-0056: Your own widgets): a composite, a function of its params written in markup. It expands to its
//! type, its params and its events as fields, and to its `Element` impl, which builds its tree from them where a
//! screen's view places it, so a screen writes a composite's element as it writes a primitive's.
//!
//! ```text
//! widget! { struct ItemSlot { item: Item } raises click; Button { click => click, … } }
//!     ──> pub struct ItemSlot<E> { pub item: Item, pub click: E }
//!     ──> impl<E: Event> Element for ItemSlot<E> { fn node(…) -> Node; fn styled(…) -> Node }
//! ```
//!
//! A composite has no state and no `self`: its references are its params and the locals its `as` makes, and the macro
//! refuses any other name, since it sees its own declarations.

mod check;
mod emit;
mod parse;

use crate::markup::Element;
use proc_macro2::TokenStream;
use syn::{Ident, ItemStruct};

/// A `widget!`'s input: the composite's type, the events it raises, and the root of its markup.
struct Composite {
    item: ItemStruct,
    raises: Vec<Ident>,
    root: Element,
}

/// Expands `widget!`'s input.
pub fn expand(input: TokenStream) -> TokenStream {
    let expanded = syn::parse2::<Composite>(input).and_then(|composite| {
        let params = check::composite(&composite)?;
        Ok(emit::composite(&composite, &params))
    });

    expanded.unwrap_or_else(syn::Error::into_compile_error)
}
