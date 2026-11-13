//! The impl `#[events]` marks, as it was written but for a lint each method it declares is spared, and the hidden trait
//! that declares its methods' arguments.

use super::check::Methods;
use super::{alias, arguments};
use quote::quote;
use syn::{ImplItem, parse_quote};

/// `methods`' impl, and the trait that declares their arguments, implemented for the screen.
pub fn events(methods: &Methods) -> proc_macro2::TokenStream {
    let Methods { block, screen, declared } = methods;
    let mut block = block.clone();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let called = declared.iter().any(|declared| declared.method == function.sig.ident);
        if called {
            function.attrs.push(parse_quote!(
                #[allow(clippy::unnecessary_wraps, reason = "an event's method returns what its call takes, failing or not")]
            ));
        }
    }
    let trait_name = arguments(screen);
    let self_type = &block.self_ty;
    let aliases: Vec<_> =
        declared.iter().flat_map(|declared| (0..declared.types.len()).map(|at| alias(&declared.method, at))).collect();
    let types = declared.iter().flat_map(|declared| &declared.types);

    quote! {
        #block

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        pub trait #trait_name {
            #(type #aliases;)*
        }

        impl #trait_name for #self_type {
            #(type #aliases = #types;)*
        }
    }
}
