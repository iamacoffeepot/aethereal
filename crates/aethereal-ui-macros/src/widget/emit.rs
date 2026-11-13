//! A composite written out: its type, its `Element` impl, which builds its root where its screen places it, and, when
//! it raises events, its `Raising` impl, which names the type its events convert to.
//!
//! ```text
//! pub struct ItemSlot<E> { pub item: Item, pub click: E }
//!
//! impl<E: Event> Raising for ItemSlot<E> { type Event = E; }
//!
//! impl<E: Event> Element for ItemSlot<E> {
//!     fn node(self, segment, children) -> Node { Element::styled(self, segment, <its root's style>, children) }
//!     fn styled(self, segment, style, appended) -> Node {
//!         let Self { item, click } = self;
//!         <its root, at `segment`, styled `style`, its own children and then `appended`>
//!     }
//! }
//! ```
//!
//! A composite's event type is the screen's, `E`, as a stock widget's is (`Button<E>`): the screen's view sets each of
//! its `raises` to a variant of the screen's event enum, and the composite passes it on unchanged, so what its widget
//! raises is the screen's own event. It builds every item of a list it holds, as it has no screen to ask which rows
//! one shows, so `Element::WINDOWS` is left `false`.

use super::Composite;
use crate::markup::{Emitter, Owner, Scope};
use crate::spans;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Fields, Ident, Visibility};

/// The expansion of `composite`, whose params are `params`.
pub fn composite(composite: &Composite, params: &[Ident]) -> TokenStream {
    let Composite { item, raises, root } = composite;
    let name = &item.ident;
    let attributes = &item.attrs;
    let visibility = &item.vis;
    let fields: Vec<TokenStream> = match &item.fields {
        Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let attributes = &field.attrs;
                let ident = &field.ident;
                let kind = &field.ty;
                let visibility = match &field.vis {
                    Visibility::Inherited => quote!(pub),
                    written => quote!(#written),
                };
                quote!(#(#attributes)* #visibility #ident: #kind)
            })
            .collect(),
        Fields::Unit | Fields::Unnamed(_) => Vec::new(),
    };
    let raised = raises.iter().map(|raise| quote!(pub #raise: E));
    let (declared, implemented, applied, raising) = if raises.is_empty() {
        (quote!(), quote!(), quote!(), quote!())
    } else {
        let raising = quote! {
            impl<E: ::aethereal_ui::Event> ::aethereal_ui::Raising for #name<E> {
                type Event = E;
            }
        };
        (quote!(<E>), quote!(<E: ::aethereal_ui::Event>), quote!(<E>), raising)
    };
    let [segment, style, appended, children] =
        ["segment", "style", "appended", "children"].map(|name| format_ident!("{name}", span = spans::hidden()));
    let own = Emitter::look(&root.look);
    let tree = Emitter::new(Owner::Composite).composite(root, &Scope::of(params), &segment, &style, &appended);

    quote! {
        #(#attributes)*
        #visibility struct #name #declared {
            #(#fields,)*
            #(#raised,)*
        }

        #raising

        impl #implemented ::aethereal_ui::Element for #name #applied {
            fn node(
                self,
                #segment: ::aethereal_ui::Segment,
                #children: ::std::vec::Vec<::aethereal_ui::Node>,
            ) -> ::aethereal_ui::Node {
                ::aethereal_ui::Element::styled(self, #segment, #own, #children)
            }

            #[allow(clippy::clone_on_copy, clippy::redundant_clone, clippy::vec_init_then_push)]
            fn styled(
                self,
                #segment: ::aethereal_ui::Segment,
                #style: ::aethereal_ui::Style,
                #appended: ::std::vec::Vec<::aethereal_ui::Node>,
            ) -> ::aethereal_ui::Node {
                #[allow(unused_variables)]
                let Self { #(#params,)* #(#raises,)* } = self;
                #tree
            }
        }
    }
}
