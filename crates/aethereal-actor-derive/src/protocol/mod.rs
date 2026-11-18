//! `#[protocol(…)]`: a named protocol, its kinds and what each is answered with, written as one attribute on its type
//! (ADR-0016, ADR-0059).
//!
//! ```text
//! #[protocol(Exited, Gather(answers = Gathered))] struct P<..> ──> impl Protocol for P { type Kinds = (Exited, Gather); }
//!                                                              ├─> impl Carries<Exited> for P { type Answers = (); }
//!                                                              └─> impl Carries<Gather> for P { type Answers = (Gathered,); }
//! ```
//!
//! The struct's generics are copied onto each impl, so a protocol's bound is written on the struct.

mod parse;

use parse::{Answers, Carried};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::ItemStruct;

/// The struct `item` as written, and the impls `#[protocol(attribute)]` writes for it.
pub fn expand(attribute: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let listed = syn::parse2::<parse::Listed>(attribute)?.carried;
    let item = syn::parse2::<ItemStruct>(item)?;
    once_each(&listed)?;
    if listed.is_empty() {
        return Err(syn::Error::new_spanned(&item.ident, "a protocol lists its kinds, `#[protocol(Kind, …)]`"));
    }

    let name = &item.ident;
    let (impl_generics, type_generics, where_clause) = item.generics.split_for_impl();
    let kinds = listed.iter().map(|carried| &carried.kind);
    let carries = listed.iter().map(|Carried { kind, answers }| {
        let answers = match answers {
            Answers::Told => quote!(()),
            Answers::Asked(answer) => quote!((#answer,)),
        };

        quote! {
            impl #impl_generics ::aethereal_actor::Carries<#kind> for #name #type_generics #where_clause {
                type Answers = #answers;
            }
        }
    });

    Ok(quote! {
        #item

        impl #impl_generics ::aethereal_actor::Protocol for #name #type_generics #where_clause {
            type Kinds = (#(#kinds,)*);
        }

        #(#carries)*
    })
}

/// Refuses a kind listed twice.
fn once_each(carried: &[Carried]) -> syn::Result<()> {
    let mut seen: Vec<String> = Vec::new();
    for Carried { kind, .. } in carried {
        let key = kind.to_token_stream().to_string();
        if seen.contains(&key) {
            return Err(syn::Error::new_spanned(
                kind,
                format!("`{key}` is listed already: a protocol lists a kind once"),
            ));
        }
        seen.push(key);
    }

    Ok(())
}
