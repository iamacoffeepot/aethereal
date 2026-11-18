//! `Receives<H>`: an `#[answer]`, an `#[on]`, an `#[ended]`, or an `#[others]`, each what the engine brings, which no
//! guard checks (ADR-0058, ADR-0059).

use super::super::block::Block;
use super::super::handler::Handler;
use super::as_trait_method;
use proc_macro2::TokenStream;
use quote::quote;

/// `Receives<H>` for `handler`, of `block`.
pub fn emit(block: &Block, handler: &Handler) -> TokenStream {
    let Block { attrs, generics, self_ty, .. } = block;
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let received = handler.listed();
    let mut method = as_trait_method(&handler.method, "handle");
    if !handler.takes_turn() {
        method.sig.inputs.push(syn::parse_quote!(_: &mut ::aethereal_actor::Turn<'_, Self>));
    }

    quote! {
        #(#attrs)*
        impl #impl_generics ::aethereal_actor::Receives<#received> for #self_ty #where_clause {
            #method
        }
    }
}
