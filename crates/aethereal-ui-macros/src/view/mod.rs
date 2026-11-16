//! `view!` (ADR-0056: Events call the screen's methods; What the markup compiles into; Named nodes): a screen's
//! markup, expanded to its event enum, its node enum when it names any node, one `impl View`, and the call of `screen!`,
//! and nothing else, so nothing generated shares an impl with what is written by hand.
//!
//! ```text
//! view! { Inventory, ui, Column { … Button { click => self.equip(item.id) } … TextInput { name: search, … } } }
//!     ──> pub enum InventoryEvent { Equip(<Inventory as __InventoryEventArgs>::equip_0) }
//!     ──> pub enum InventoryNode { Search }, whose `path()` is the key path the view builds it at
//!     ──> impl View for Inventory { type Event; fn view(&self) -> Tree; fn on(&mut self, turn, raised) }
//!     ──> aethereal_ui::screen!(Inventory, ui);
//! ```

mod check;
mod emit;
mod parse;

use crate::markup::{self, Element, Scope};
use proc_macro2::TokenStream;
use syn::Ident;

/// A `view!`'s input: the screen's type, the field that holds its `Screen`, and the root of its markup.
struct View {
    screen: Ident,
    field: Ident,
    root: Element,
}

/// Expands `view!`'s input.
pub fn expand(input: TokenStream) -> TokenStream {
    let expanded = syn::parse2::<View>(input).and_then(|view| {
        markup::check(&view.root, &check::Screen, &Scope::default())?;
        let variants = check::variants(&view.root)?;
        check::names(&view.root)?;
        Ok(emit::view(&view, &variants))
    });

    expanded.unwrap_or_else(syn::Error::into_compile_error)
}
