//! The derives of `aethereal-data`: `Plain`, `Bound`, and `Key` (ADR-0004), and a kind named on the first two (ADR-0006).

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
/// field's type. Every field must be `Plain`. `#[kind(name = "…")]` on the
/// type also writes `Kind` (ADR-0006), and `#[section(name = "…")]` on a
/// struct writes `Section`, a handle for each named field, and the flags
/// `#[flag]` suggests (ADR-0028).
#[proc_macro_derive(Plain, attributes(plain, kind, section, flag))]
pub fn derive_plain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match plain::expand(&input, plain::Derive::Plain) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives `Schema` and the codec from one field list, for a type that holds a bound value (ADR-0004).
///
/// It writes the codec `Plain` writes, over every boundary and bounded by each
/// field's type, so the type crosses exactly the boundaries all its fields
/// cross. It does not state `Plain`. `#[kind(name = "…")]` on the type also
/// writes `Kind` (ADR-0006), and `#[section(name = "…")]` writes `Section`
/// as it does on `Plain` (ADR-0028).
#[proc_macro_derive(Bound, attributes(plain, kind, section, flag))]
pub fn derive_bound(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match plain::expand(&input, plain::Derive::Bound) {
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
