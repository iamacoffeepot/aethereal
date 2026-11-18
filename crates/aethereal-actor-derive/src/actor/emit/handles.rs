//! `Handles<K>`: a `#[request]` or a `#[tell]`, its guard and its answer read from its signature, with its kind's
//! `#[refused]` as its `refused` (ADR-0058, ADR-0059).

use super::super::block::Block;
use super::super::handler::Handler;
use super::as_trait_method;
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

/// `Handles<K>` for `handler`, of `block`.
pub fn emit(block: &Block, handler: &Handler) -> TokenStream {
    let Block { attrs, generics, self_ty, .. } = block;
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let kind = handler.listed();
    let guard = handler.guard();
    let answer = &handler.answer;
    let mut method = as_trait_method(&handler.method, "handle");
    if !handler.takes_turn() {
        method.sig.inputs.push(syn::parse_quote!(_: &mut ::aethereal_actor::Turn<'_, Self>));
    }
    let refused = block.refusals.iter().find(|refusal| handler.pairs(refusal)).map(refused);

    quote! {
        #(#attrs)*
        impl #impl_generics ::aethereal_actor::Handles<#kind> for #self_ty #where_clause {
            type From = #guard;
            type Returns = #answer;

            #method

            #refused
        }
    }
}

/// `refused`, which calls `refusal`, the block's `#[refused]` method of the kind, as written, and answers what it
/// returns in its handler's form. A refusal that returns another form than its handler's is pointed at its return type.
fn refused(refusal: &Handler) -> TokenStream {
    let name = &refusal.method.sig.ident;
    let kind = refusal.listed();
    let (turn, call) = if refusal.takes_turn() {
        (quote!(turn), quote!(Self::#name(self, mail, turn)))
    } else {
        (quote!(_), quote!(Self::#name(self, mail)))
    };
    let answered = quote_spanned! { refusal.answer.span() => #call.map(::aethereal_actor::Refusal::Answers) };

    quote! {
        fn refused(
            &mut self,
            mail: #kind,
            #turn: &mut ::aethereal_actor::Turn<'_, Self>,
        ) -> ::core::result::Result<::aethereal_actor::Refusal<Self::Returns>, ::aethereal_actor::Fault> {
            #answered
        }
    }
}
