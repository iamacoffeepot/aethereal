//! What an actor block writes: `ChildOf` and `Actor`, one impl for each handler, and `impl Name` of the rest
//! (ADR-0059).
//!
//! Each method the block holds keeps its body as written, with `Spawn` brought into scope at its top, so a spawn in it,
//! `turn.spawn::<C>(…)`, builds with no import of its own: a spawn is a method of the trait `Spawn<Me, I>`, whose place
//! `I` is inferred at the call, which no inherent method could take while `C` is named (ADR-0059).

mod actor;
mod handles;
mod receives;

use super::block::Block;
use super::options::Options;
use super::role::Role;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{ImplItem, ImplItemFn, Visibility, parse_quote};

/// Everything `block` writes, with `options`.
pub fn emit(block: &Block, options: &Options) -> TokenStream {
    let actor = actor::emit(block, options);
    let handlers = block.handlers.iter().map(|handler| match handler.role {
        Role::Request | Role::Tell => handles::emit(block, handler),
        Role::Answer | Role::On | Role::Ended | Role::Others => receives::emit(block, handler),
        // A refusal is kept apart, and written into its pair's `Handles`.
        Role::Refused => TokenStream::new(),
    });
    let ordinary = ordinary(block);

    quote! {
        #actor
        #(#handlers)*
        #ordinary
    }
}

/// `impl Name` of what carries no role word but `begin` and `end`, and of each `#[refused]` method as written, which
/// its kind's `refused` calls; nothing when there is none.
fn ordinary(block: &Block) -> TokenStream {
    let refusals = block.refusals.iter().map(|refusal| {
        let mut method = spawning(refusal.method.clone());
        // Its signature is its pair's, so whether it reads `self` or always answers is not its own to choose.
        method.attrs.push(parse_quote!(#[allow(clippy::unnecessary_wraps, clippy::unused_self)]));

        ImplItem::Fn(method)
    });
    let ordinary = block.ordinary.iter().cloned().map(|item| match item {
        ImplItem::Fn(method) => ImplItem::Fn(spawning(method)),
        other => other,
    });
    let items: Vec<ImplItem> = ordinary.chain(refusals).collect();
    if items.is_empty() {
        return TokenStream::new();
    }
    let Block { attrs, generics, self_ty, .. } = block;
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    quote! {
        #(#attrs)*
        impl #impl_generics #self_ty #where_clause {
            #(#items)*
        }
    }
}

/// `method`, written as a trait's: named `name`, with no visibility of its own, and spawning.
fn as_trait_method(method: &ImplItemFn, name: &str) -> ImplItemFn {
    let mut method = spawning(method.clone());
    method.vis = Visibility::Inherited;
    method.sig.ident = syn::Ident::new(name, method.sig.ident.span());

    method
}

/// `method` with `Spawn` brought into scope at the top of its body.
fn spawning(mut method: ImplItemFn) -> ImplItemFn {
    method.block.stmts.insert(
        0,
        parse_quote! {
            #[allow(unused_imports)]
            use ::aethereal_actor::Spawn as _;
        },
    );

    method
}
