//! `ChildOf` and `Actor`: where the actor stands, its config and key, the lists it declares, its `begin`, and its `end`
//! (ADR-0022, ADR-0059).

use super::super::block::{Block, End};
use super::super::handler::Handler;
use super::super::lists;
use super::super::options::{Library, Options};
use super::as_trait_method;
use proc_macro2::TokenStream;
use quote::quote;

/// `ChildOf` and `Actor` for `block`, with `options`.
pub fn emit(block: &Block, options: &Options) -> TokenStream {
    let Block { attrs, generics, self_ty, begin, end, handlers, .. } = block;
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let Options { key, parent, depends, spawns, library } = options;

    let listed: Vec<TokenStream> = handlers.iter().map(Handler::listed).collect();
    let (handles, depends) = match library {
        Library::Own => (lists::tuple(&listed), lists::types(depends, generics)),
        Library::With(library) => {
            let depends = lists::entries(depends, generics);
            (quote!(#library::handles!(#(#listed),*)), quote!(#library::depends!(#(#depends),*)))
        }
    };
    let spawns = lists::types(spawns, generics);
    let config = &begin.config;
    let begin = as_trait_method(&begin.method, "begin");
    let end = match end {
        End::Written(method) => {
            let method = as_trait_method(method, "end");
            quote!(#method)
        }
        End::Default => quote! {
            fn end(
                self,
                _: &mut ::aethereal_actor::Ending<'_, Self>,
            ) -> ::core::result::Result<(), ::aethereal_actor::Fault> {
                ::core::result::Result::Ok(())
            }
        },
    };

    quote! {
        #(#attrs)*
        impl #impl_generics ::aethereal_actor::ChildOf for #self_ty #where_clause {
            type Parent = #parent;
        }

        #(#attrs)*
        impl #impl_generics ::aethereal_actor::Actor for #self_ty #where_clause {
            type Config = #config;
            type Key = #key;
            type Handles = #handles;
            type Depends = #depends;
            type Spawns = #spawns;

            #begin

            #end
        }
    }
}
