//! `#[actor]`: an actor written as one block (ADR-0059).
//!
//! ```text
//! #[actor(options)] impl Name { … } ──> options: key, parent, depends, spawns, with
//!                                   └─> block: begin, end, each handler by its role word, the other items
//!                                         ──> ChildOf and Actor, one impl for each handler, and impl Name of the rest
//! ```

mod block;
mod emit;
mod handler;
mod lists;
mod options;
mod role;

use block::Block;
use options::Options;
use proc_macro2::TokenStream;
use syn::ItemImpl;

/// The impls `#[actor(attribute)]` writes for the block `item`.
pub fn expand(attribute: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let options = Options::parse(attribute)?;
    let block = Block::read(syn::parse2::<ItemImpl>(item)?)?;

    Ok(emit::emit(&block, &options))
}
