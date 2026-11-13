//! `#[events]`: the types of the arguments a screen's methods take, declared for its `view!`. A variant of the event
//! enum holds the arguments its event copies, so it is written with their types; the markup writes only their values,
//! `self.select(item.id)`, and one macro cannot read another's input, so the impl that holds the methods declares
//! them, as a hidden trait `view!` names by the screen's type.
//!
//! ```text
//! #[events] impl Inventory { fn select(&mut self, turn: &mut Turn<…>, item: ItemId) -> Result<(), Fault> }
//!     ──> trait __InventoryEventArgs { type select_0; }  impl __InventoryEventArgs for Inventory { type select_0 = ItemId; }
//! ```
//!
//! Each method that takes `&mut self` and a turn declares each parameter after them, whether the view copies it or a
//! widget hands it: the markup says how many it copies.

mod check;
mod emit;

use proc_macro2::{Ident, TokenStream};
use quote::format_ident;

/// Expands `#[events]` on `item`, an inherent impl of a screen.
pub fn expand(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let expanded = check::methods(attribute, item).map(|methods| emit::events(&methods));

    expanded.unwrap_or_else(syn::Error::into_compile_error)
}

/// The hidden trait that declares `screen`'s methods' arguments.
pub fn arguments(screen: &Ident) -> Ident {
    format_ident!("__{screen}EventArgs")
}

/// The associated type of the argument at `at` of `method`, counted from the first after the turn.
pub fn alias(method: &Ident, at: usize) -> Ident {
    format_ident!("{method}_{at}", span = method.span())
}
