//! The derives of `aethereal-data`: `Plain` and `Key` (ADR-0004).

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod key;
mod lifetimes;
mod plain;

/// Derives `Schema`, the codec, and `Plain` from one field list (ADR-0004).
///
/// A struct becomes a `Struct` shape with its fields in declared order;
/// an enum becomes an `Enum` shape with a selector before each variant's
/// fields. The codec is written over every boundary and bounded by each
/// field's type. Every field must be `Plain`.
#[proc_macro_derive(Plain, attributes(plain))]
pub fn derive_plain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match plain::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives `Key` for a struct with exactly one field (ADR-0004).
///
/// The comparison impls delegate to the single field, so the struct orders
/// like its field does.
#[proc_macro_derive(Key)]
pub fn derive_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match key::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
