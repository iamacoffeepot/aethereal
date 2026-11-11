//! The derives of `aethereal-data`: `Plain`, `Bound`, and `Key` (ADR-0004), and a kind named on the first two (ADR-0006);
//! `Bootstrap`, which the substrate re-exports, for an actor whose config is its settings (ADR-0029); and `Portability`,
//! which the actor crate re-exports, for a state read by field name into its next version's form (ADR-0054).

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod bootstrap;
mod key;
mod lifetimes;
mod plain;
mod portability;

/// Derives `Schema`, the codec, and `Plain` from one field list (ADR-0004).
///
/// A struct becomes a `Struct` shape with its fields in declared order;
/// an enum becomes an `Enum` shape with a selector before each variant's
/// fields. The codec is written over every boundary and bounded by each
/// field's type. Every field must be `Plain`. `#[kind(name = "…")]` on the
/// type also writes `Kind` (ADR-0006), and `#[settings]` on a struct
/// writes `Settings`, a handle for each named field, and the flags
/// `#[flag]` suggests (ADR-0028). `#[flag]` on a field of any other struct
/// writes `Flagged`, the command line a tool's input is typed with, where
/// `#[flag(position)]` also fills a field by the next bare word (ADR-0053).
#[proc_macro_derive(Plain, attributes(plain, kind, settings, flag))]
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
/// writes `Kind` (ADR-0006), and `#[settings]` writes `Settings` as it
/// does on `Plain` (ADR-0028).
#[proc_macro_derive(Bound, attributes(plain, kind, settings, flag))]
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

/// Derives `Bootstrap<H>` for every host `H`, for an actor whose config is its settings (ADR-0029).
///
/// The actor's config is plain data with `#[settings]`, read from the section its kind name's last segment names
/// (`aether.example.scene` reads `scene`); a program lists it with `.root` when the config has a `Default`, and with
/// `.root_with` otherwise. It needs nothing, provides nothing, and begins as a singleton
/// from its settings. Use it through `aethereal_substrate::Bootstrap`, which re-exports it beside the trait.
#[proc_macro_derive(Bootstrap, attributes(kind))]
pub fn derive_bootstrap(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match bootstrap::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives `Portability` for a state struct: its portable form, a struct named `Portable` and the state's name, of its
/// fields under the same names and visibilities, each an `Option` of the field's type, read by field name (ADR-0054).
///
/// Use it through `aethereal_actor::Portability`, which re-exports it beside the trait.
#[proc_macro_derive(Portability)]
pub fn derive_portability(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match portability::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
