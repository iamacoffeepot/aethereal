//! `#[derive(Portability)]`: a state's portable form, its fields under their names each an `Option`, which the next
//! version of its code reads by field name (ADR-0054). `aethereal-actor` re-exports it beside the trait.

use crate::lifetimes::reject_lifetimes;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields};

/// The portable form of the state `input` and `impl Portability` linking the two: a struct named `Portable` and the
/// state's name, of the state's fields under the same names and visibilities, each an `Option` of the field's type.
pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    reject_lifetimes(&input.generics, "Portability")?;
    let Data::Struct(data) = &input.data else {
        let message = "`Portability` reads a state's fields by name: derive it on a struct";
        return Err(syn::Error::new_spanned(&input.ident, message));
    };

    let name = &input.ident;
    let vis = &input.vis;
    let form = format_ident!("Portable{name}");
    let form_doc = format!(
        "The portable form of `{name}`: each of its fields, absent when the state it was read from had none of that \
         name (ADR-0054)."
    );
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let predicates = where_clause.map(|clause| &clause.predicates);
    let field_types: Vec<_> = data.fields.iter().map(|field| &field.ty).collect();

    let (definition, construction) = match &data.fields {
        Fields::Named(named) => {
            let mut declared = Vec::new();
            let mut read = Vec::new();
            for field in &named.named {
                let (ident, ty, field_vis) = (&field.ident, &field.ty, &field.vis);
                let label = ident.as_ref().map(ToString::to_string).unwrap_or_default();
                let doc = format!("`{label}`, when the old state had it.");
                declared.push(quote! { #[doc = #doc] #field_vis #ident: ::core::option::Option<#ty> });
                read.push(quote! { #ident: fields.read(#label)? });
            }
            let definition = quote! { #vis struct #form #impl_generics #where_clause { #( #declared ),* } };
            let construction = quote! {
                let mut fields = old.fields()?;
                ::core::result::Result::Ok(#form { #( #read ),* })
            };
            (definition, construction)
        }
        Fields::Unnamed(unnamed) => {
            let mut declared = Vec::new();
            let mut read = Vec::new();
            for (position, field) in unnamed.unnamed.iter().enumerate() {
                let (ty, field_vis) = (&field.ty, &field.vis);
                let label = position.to_string();
                let doc = format!("Field `{label}`, when the old state had it.");
                declared.push(quote! { #[doc = #doc] #field_vis ::core::option::Option<#ty> });
                read.push(quote! { fields.read(#label)? });
            }
            let definition = quote! { #vis struct #form #impl_generics ( #( #declared ),* ) #where_clause; };
            let construction = quote! {
                let mut fields = old.fields()?;
                ::core::result::Result::Ok(#form ( #( #read ),* ))
            };
            (definition, construction)
        }
        Fields::Unit => {
            let definition = quote! { #vis struct #form; };
            let construction = quote! {
                old.fields()?;
                ::core::result::Result::Ok(#form)
            };
            (definition, construction)
        }
    };

    Ok(quote! {
        #[doc = #form_doc]
        #definition

        #[automatically_derived]
        impl #impl_generics ::aethereal_actor::Portability for #name #ty_generics
        where
            #( #field_types: ::aethereal_actor::__derive::Decode<::aethereal_actor::Within> + ::aethereal_actor::__derive::Schema, )*
            #predicates
        {
            type Form = #form #ty_generics;

            fn read(
                old: &::aethereal_actor::OldState<'_>,
            ) -> ::core::result::Result<Self::Form, ::aethereal_actor::TakeOverError> {
                #construction
            }
        }
    })
}
