//! A screen's markup written out: its event enum, its `impl View`, and the call of `screen!`.
//!
//! ```text
//! pub enum InventoryEvent { Select(<Inventory as __InventoryEventArgs>::select_0), Place(…), Discard }
//!
//! fn on(&mut self, turn, raised) {
//!     match raised.event() {
//!         InventoryEvent::Select(a0) => Method::call(Self::select, self, turn, (a0,), raised),   // + what it hands
//!         InventoryEvent::Place(a0) => match raised {                                          // + the payload
//!             Raised::Dropped(_, payload) => dropped(turn, &payload, |turn, payload| self.place(turn, a0, payload)),
//!             unmatched => unmatched(turn, &unmatched),
//!         },
//!     }
//! }
//! ```
//!
//! A variant's arguments are typed by its method, as `#[events]` declares them, since the macro sees the markup alone.
//! What a widget hands with its event is read by the method's last parameter, through `Method`: rustc picks the call
//! with the parameters the method has, so a method that takes the hand gets it, and an event raised with another hand
//! than the method takes is unmatched.

use super::View;
use super::check::{Taking, Variant};
use crate::events;
use crate::markup::{Emitter, Named, Owner, variant};
use crate::spans;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};

/// The expansion of `view`, whose events call `variants`.
pub fn view(view: &View, variants: &[Variant]) -> TokenStream {
    let View { screen, field, root } = view;
    let events = format_ident!("{screen}Event", span = screen.span());
    let arguments = events::arguments(screen);
    let definitions = variants.iter().map(|variant| {
        let name = self::variant(&variant.method);
        let types = (0..variant.arguments).map(|at| {
            let alias = events::alias(&variant.method, at);
            quote_spanned!(spans::at(variant.method.span())=> <#screen as #arguments>::#alias)
        });
        if variant.arguments == 0 {
            quote!(#name)
        } else {
            quote!(#name(#(#types),*))
        }
    });
    let arms = variants.iter().map(|variant| arm(&events, variant));
    let empty = variants.is_empty();
    let quiet = if empty {
        quote!(let _ = turn;)
    } else {
        quote!()
    };
    let emitter = Emitter::new(Owner::Screen { events: &events, field });
    let tree = emitter.root(root);
    let nodes = nodes(screen, &emitter.into_named());
    let documented =
        format!("The events of `{screen}`'s markup, one for each method they call, as its `view!` wrote it.");

    quote! {
        #[doc = #documented]
        #[derive(Debug, Clone, Copy, PartialEq)]
        #[allow(clippy::derive_partial_eq_without_eq)]
        pub enum #events {
            #(#definitions,)*
        }

        impl ::aethereal_ui::View for #screen {
            type Event = #events;

            #[allow(clippy::clone_on_copy, clippy::redundant_clone, clippy::vec_init_then_push)]
            fn view(&self) -> ::aethereal_ui::Tree {
                ::aethereal_ui::Tree::new(#tree)
            }

            fn on(
                &mut self,
                turn: &mut ::aethereal_ui::__private::Turn<'_, Self, <Self as ::aethereal_ui::Controls>::Sends>,
                raised: ::aethereal_ui::Raised<#events>,
            ) -> ::core::result::Result<(), ::aethereal_ui::__private::Fault> {
                #quiet
                match ::aethereal_ui::Raised::event(&raised) {
                    #(#arms)*
                }
            }
        }

        #nodes

        ::aethereal_ui::screen!(#screen, #field);
    }
}

/// The node enum of `screen`, whose markup names `named`: a variant for each, holding the key of each item it stands
/// within, outermost first, and the key path the view builds it at; nothing when the markup names no node.
fn nodes(screen: &syn::Ident, named: &[Named]) -> TokenStream {
    if named.is_empty() {
        return TokenStream::new();
    }
    let nodes = format_ident!("{screen}Node", span = screen.span());
    let definitions = named.iter().map(|Named { name, keys, .. }| {
        let variant = variant(name);
        let documented = format!("The node `{screen}`'s markup names `{name}`.");
        let types = keys.iter().map(|_| quote!(::aethereal_ui::ItemKey));
        if keys.is_empty() {
            quote!(#[doc = #documented] #variant)
        } else {
            quote!(#[doc = #documented] #variant(#(#types),*))
        }
    });
    let arms = named.iter().map(|Named { name, path, keys }| {
        let variant = variant(name);
        let pattern = if keys.is_empty() {
            quote!(Self::#variant)
        } else {
            quote!(Self::#variant(#(#keys),*))
        };
        quote!(#pattern => ::aethereal_ui::KeyPath::of(&[#(#path),*]),)
    });
    let documented = format!(
        "The nodes `{screen}`'s markup names, each the variant of its name, holding the key of each item it stands \
         within, outermost first, as its `view!` wrote it."
    );

    quote! {
        #[doc = #documented]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum #nodes {
            #(#definitions,)*
        }

        impl #nodes {
            /// Its key path in the tree the view builds: where the screen lays it out, keeps its state, and gives it
            /// focus, while it is shown.
            #[must_use]
            pub fn path(self) -> ::aethereal_ui::KeyPath {
                match self {
                    #(#arms)*
                }
            }
        }
    }
}

/// The arm of `on` that calls `variant`'s method.
fn arm(events: &syn::Ident, variant: &Variant) -> TokenStream {
    let method = &variant.method;
    let name = self::variant(method);
    let span = spans::at(method.span());
    let copied: Vec<_> = (0..variant.arguments).map(|at| format_ident!("a{at}", span = spans::hidden())).collect();
    let pattern = if copied.is_empty() {
        quote!(#events::#name)
    } else {
        quote!(#events::#name(#(#copied),*))
    };
    match variant.taking {
        Taking::Handed => quote_spanned!(span=>
            #pattern => ::aethereal_ui::Method::call(Self::#method, self, turn, (#(#copied,)*), raised),
        ),
        Taking::Payload => quote_spanned!(span=>
            #pattern => match raised {
                ::aethereal_ui::Raised::Dropped(_, payload) => {
                    ::aethereal_ui::dropped(turn, &payload, |turn, payload| self.#method(turn, #(#copied,)* payload))
                }
                unmatched => ::aethereal_ui::unmatched(turn, &unmatched),
            },
        ),
    }
}
