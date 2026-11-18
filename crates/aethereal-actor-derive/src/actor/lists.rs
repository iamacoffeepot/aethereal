//! The lists an actor's `Actor` impl holds, as the block writes them (ADR-0059).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{GenericParam, Generics, Type};

/// A list of actor types, as an actor's `Depends` or `Spawns` holds one, of `entries`: `()` for none, the one type for
/// one, and a tuple for more. An entry that is a type parameter of the block, `generics`, counts as the types it lists,
/// `Each<L>`, as a list type parameter does; one that stands for a single type is a list of that one.
pub fn types(entries: &[Type], generics: &Generics) -> TokenStream {
    let entries: Vec<TokenStream> = entries.iter().map(|entry| entry_of(entry, generics)).collect();

    match entries.as_slice() {
        [] => quote!(()),
        [one] => one.clone(),
        _ => quote!((#(#entries,)*)),
    }
}

/// The entries of a list, each as [`types`] writes it, for a library's `depends!` to take in.
pub fn entries(entries: &[Type], generics: &Generics) -> Vec<TokenStream> {
    entries.iter().map(|entry| entry_of(entry, generics)).collect()
}

/// A tuple of `entries`, as an actor's `Handles` is one, `()` for none.
pub fn tuple(entries: &[TokenStream]) -> TokenStream {
    quote!((#(#entries,)*))
}

/// `entry`, or `Each<entry>` when it is a type parameter of `generics`.
fn entry_of(entry: &Type, generics: &Generics) -> TokenStream {
    let Type::Path(path) = entry else {
        return quote!(#entry);
    };
    let Some(ident) = path.path.get_ident() else {
        return quote!(#entry);
    };
    let is_parameter = generics.params.iter().any(|parameter| match parameter {
        GenericParam::Type(parameter) => parameter.ident == *ident,
        GenericParam::Lifetime(_) | GenericParam::Const(_) => false,
    });

    if is_parameter {
        quote!(::aethereal_actor::Each<#entry>)
    } else {
        quote!(#entry)
    }
}
