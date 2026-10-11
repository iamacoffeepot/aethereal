//! The `Plain` derive: one field list into a schema and a codec (ADR-0004).

mod enumeration;
mod structure;

use crate::lifetimes::reject_lifetimes;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Generics, Ident, Type};

/// Expands `#[derive(Plain)]` into `Schema` and `Plain` impls.
pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    reject_lifetimes(&input.generics, "Plain")?;

    match &input.data {
        Data::Struct(data) => structure::expand(input, data),
        Data::Enum(data) => enumeration::expand(input, data),
        Data::Union(data) => {
            let token = data.union_token;
            let message = "Plain derive does not support unions";
            Err(syn::Error::new_spanned(token, message))
        }
    }
}

/// The generics with a `Plain` bound on each type parameter.
///
/// Const parameters pass through untouched.
pub fn with_plain_bounds(generics: &Generics) -> Generics {
    let mut plain = generics.clone();

    for param in &mut plain.params {
        if let syn::GenericParam::Type(typed) = param {
            typed.bounds.push(syn::parse_quote!(::aethereal_data::Plain));
        }
    }

    plain
}

/// One `Field` entry naming `field_type`'s shape.
pub fn shape_field(name: &str, field_type: &Type) -> TokenStream2 {
    quote! {
        ::aethereal_data::Field {
            name: #name,
            shape: <#field_type as ::aethereal_data::Schema>::SHAPE,
        }
    }
}

/// Encodes the value held in `value`, spanned at the field's type.
pub fn encode_call(value: &TokenStream2, field_type: &Type) -> TokenStream2 {
    let span = field_type.span();
    quote_spanned! { span =>
        <#field_type as ::aethereal_data::Plain>::encode(#value, out)?;
    }
}

/// Decodes into `decode_ident`, spanned at the field's type.
pub fn decode_binding(decode_ident: &Ident, field_type: &Type) -> TokenStream2 {
    let span = field_type.span();
    quote_spanned! { span =>
        let #decode_ident = <#field_type as ::aethereal_data::Plain>::decode(input, budget)?;
    }
}
