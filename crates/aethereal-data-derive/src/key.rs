//! The `Key` derive: comparison impls that delegate to one field (ADR-0004).

use crate::lifetimes::reject_lifetimes;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Generics};

/// Refusals from the `Key` derive, which takes one-field structs only.
const ONLY_ONE_FIELD: &str = "Key derive accepts only a struct with exactly one field";

/// One field a `Key` struct holds, with its member access prebuilt.
struct Single {
    member: syn::Member,
    field_type: syn::Type,
}

/// Expands `#[derive(Key)]` into `Key` and comparison impls.
pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    reject_lifetimes(&input.generics, "Key")?;
    let single = single_field(input)?;

    let mut generics = input.generics.clone();
    let span = single.field_type.span();
    let field_type = &single.field_type;
    let predicate: syn::WherePredicate = syn::parse_quote_spanned!(span => #field_type: ::aethereal_data::Key);
    generics.make_where_clause().predicates.push(predicate);

    Ok(finish(input, &generics, &single))
}

/// Assembles the five impls from the generics carrying the field bound.
fn finish(input: &DeriveInput, generics: &Generics, single: &Single) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let member = &single.member;

    quote! {
        impl #impl_generics ::aethereal_data::Key for #name #ty_generics #where_clause {}

        #[automatically_derived]
        impl #impl_generics ::core::cmp::PartialEq for #name #ty_generics #where_clause {
            fn eq(&self, other: &Self) -> bool {
                self.#member == other.#member
            }
        }

        #[automatically_derived]
        impl #impl_generics ::core::cmp::Eq for #name #ty_generics #where_clause {}

        #[automatically_derived]
        impl #impl_generics ::core::cmp::PartialOrd for #name #ty_generics #where_clause {
            fn partial_cmp(
                &self,
                other: &Self,
            ) -> ::core::option::Option<::core::cmp::Ordering> {
                self.#member.partial_cmp(&other.#member)
            }
        }

        #[automatically_derived]
        impl #impl_generics ::core::cmp::Ord for #name #ty_generics #where_clause {
            fn cmp(&self, other: &Self) -> ::core::cmp::Ordering {
                self.#member.cmp(&other.#member)
            }
        }
    }
}

/// The struct's only field, or a compile error saying one field is required.
fn single_field(input: &DeriveInput) -> syn::Result<Single> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, ONLY_ONE_FIELD));
    };

    let mut fields = data.fields.iter();
    let (Some(first), None) = (fields.next(), fields.next()) else {
        return Err(syn::Error::new_spanned(&input.ident, ONLY_ONE_FIELD));
    };

    let member = first
        .ident
        .as_ref()
        .map_or_else(|| syn::Member::Unnamed(syn::Index::from(0)), |ident| syn::Member::Named(ident.clone()));
    let single = Single { member, field_type: first.ty.clone() };

    Ok(single)
}
