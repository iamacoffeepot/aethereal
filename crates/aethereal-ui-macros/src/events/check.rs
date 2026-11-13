//! Which of an impl's methods `#[events]` declares: each that takes `&mut self` and a turn, has no generics, and whose
//! parameters after them are owned types, which an event's arguments and a widget's hand are.

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::{FnArg, Ident, ImplItem, ItemImpl, Type};

/// An impl, and the methods of it whose arguments `#[events]` declares.
pub struct Methods {
    pub block: ItemImpl,
    pub screen: Ident,
    pub declared: Vec<Declared>,
}

/// One method declared: its name, and the types of its parameters after the turn.
pub struct Declared {
    pub method: Ident,
    pub types: Vec<Type>,
}

/// The methods of `item` that `#[events]` declares.
///
/// # Errors
///
/// An attribute given anything, or an impl that is not an inherent impl of a screen named by its type alone.
pub fn methods(attribute: TokenStream, item: TokenStream) -> syn::Result<Methods> {
    let given = !attribute.is_empty();
    if given {
        return Err(syn::Error::new_spanned(attribute, "`#[events]` takes nothing"));
    }
    let block: ItemImpl = syn::parse2(item)?;
    if let Some((_, path, _)) = &block.trait_ {
        return Err(syn::Error::new_spanned(path, "`#[events]` declares the methods of an inherent impl"));
    }
    let generic = !block.generics.params.is_empty();
    if generic {
        return Err(syn::Error::new_spanned(
            &block.generics,
            "`#[events]` declares the methods of a screen with no generics",
        ));
    }
    let Type::Path(named) = &*block.self_ty else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[events]` declares the methods of a screen, by its type",
        ));
    };
    let Some(screen) = named.path.segments.last().map(|last| last.ident.clone()) else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[events]` declares the methods of a screen, by its type",
        ));
    };
    let declared = block.items.iter().filter_map(declared).collect();

    Ok(Methods { block, screen, declared })
}

/// `item`'s declaration, if it is a method an event can call.
fn declared(item: &ImplItem) -> Option<Declared> {
    let ImplItem::Fn(function) = item else {
        return None;
    };
    let signature = &function.sig;
    let mutable =
        signature.receiver().is_some_and(|receiver| receiver.reference.is_some() && receiver.mutability.is_some());
    let plain = signature.generics.params.is_empty();
    let types: Vec<Type> = signature
        .inputs
        .iter()
        .skip(2)
        .filter_map(|input| match input {
            FnArg::Typed(typed) => Some((*typed.ty).clone()),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let owned = types.iter().all(owned);
    let callable = mutable && plain && owned && signature.inputs.len() >= 2;

    callable.then(|| Declared { method: signature.ident.clone(), types })
}

/// Whether `written` names an owned type, with no reference, lifetime, `impl`, or `_` in it: one a trait's associated
/// type can be.
fn owned(written: &Type) -> bool {
    let mut stack: Vec<TokenTree> = written.to_token_stream().into_iter().collect();
    while let Some(tree) = stack.pop() {
        let borrowed = match &tree {
            TokenTree::Punct(punct) => punct.as_char() == '&' || punct.as_char() == '\'',
            TokenTree::Ident(ident) => ident == "impl" || ident == "_",
            TokenTree::Group(group) => {
                stack.extend(group.stream());
                false
            }
            TokenTree::Literal(_) => false,
        };
        if borrowed {
            return false;
        }
    }

    true
}
