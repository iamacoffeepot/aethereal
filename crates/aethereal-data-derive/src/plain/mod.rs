//! The `Plain` and `Bound` derives: one field list into a schema and a codec (ADR-0004).

mod enumeration;
mod structure;

use crate::lifetimes::reject_lifetimes;
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Generics, Ident, Type};

/// Which derive is being expanded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Derive {
    /// `Plain`: the schema, the codec, and the statement that the type is only its bytes.
    Plain,
    /// `Bound`: the schema and the codec, for a type that holds a bound value.
    Bound,
}

impl Derive {
    /// The derive's name, for error messages.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "Plain",
            Self::Bound => "Bound",
        }
    }
}

/// What a struct and an enum each supply to the impls.
pub struct Codec {
    /// The expression of `Schema::SHAPE`.
    pub shape: TokenStream2,
    /// The body of `encode`, ending in its result.
    pub encode: TokenStream2,
    /// The body of `decode`, ending in its result.
    pub decode: TokenStream2,
}

/// Expands a derive into `Schema`, `Encode`, and `Decode` impls, and for `Plain` the `Plain` impl.
pub fn expand(input: &DeriveInput, derive: Derive) -> syn::Result<TokenStream2> {
    reject_lifetimes(&input.generics, derive.name())?;

    match &input.data {
        Data::Struct(data) => structure::expand(input, data, derive),
        Data::Enum(data) => enumeration::expand(input, data, derive),
        Data::Union(data) => {
            let token = data.union_token;
            let message = format!("{} derive does not support unions", derive.name());
            Err(syn::Error::new_spanned(token, message))
        }
    }
}

/// The impls of one derived type, each bounded by the type of every field.
///
/// The codec is written once over every boundary `__B`, so the type crosses
/// exactly the boundaries all its fields cross. `impl Plain`, which only the
/// `Plain` derive writes, requires every field to be `Plain`, so a field that
/// is not is an error at its type.
pub fn impls(input: &DeriveInput, field_types: &[&Type], codec: &Codec, derive: Derive) -> TokenStream2 {
    let name = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let over_boundary = over_boundary(&input.generics);
    let schema_generics = with_field_bounds(&input.generics, field_types, &quote! { ::aethereal_data::Schema });
    let encode_generics = with_field_bounds(&over_boundary, field_types, &quote! { ::aethereal_data::Encode<__B> });
    let decode_generics = with_field_bounds(&over_boundary, field_types, &quote! { ::aethereal_data::Decode<__B> });
    let plain_generics = with_field_bounds(&input.generics, field_types, &quote! { ::aethereal_data::Plain });
    let (schema_impl, _, schema_where) = schema_generics.split_for_impl();
    let (encode_impl, _, encode_where) = encode_generics.split_for_impl();
    let (decode_impl, _, decode_where) = decode_generics.split_for_impl();
    let (plain_impl, _, plain_where) = plain_generics.split_for_impl();
    let Codec { shape, encode, decode } = codec;
    let plain = match derive {
        Derive::Plain => quote! {
            #[automatically_derived]
            impl #plain_impl ::aethereal_data::Plain for #name #ty_generics #plain_where {}
        },
        Derive::Bound => TokenStream2::new(),
    };

    quote! {
        #[automatically_derived]
        impl #schema_impl ::aethereal_data::Schema for #name #ty_generics #schema_where {
            const SHAPE: ::aethereal_data::Shape = #shape;
        }

        #[automatically_derived]
        impl #encode_impl ::aethereal_data::Encode<__B> for #name #ty_generics #encode_where {
            fn encode(
                &self,
                out: &mut ::aethereal_data::Writer<'_, __B>,
            ) -> ::core::result::Result<(), ::aethereal_data::EncodeError> {
                #encode
            }
        }

        #[automatically_derived]
        impl #decode_impl ::aethereal_data::Decode<__B> for #name #ty_generics #decode_where {
            fn decode(
                input: &mut ::aethereal_data::Reader<'_, __B>,
            ) -> ::core::result::Result<Self, ::aethereal_data::DecodeError> {
                #decode
            }
        }

        #plain
    }
}

/// The generics with the boundary parameter `__B` added after the type's own.
fn over_boundary(generics: &Generics) -> Generics {
    let mut over = generics.clone();
    over.params.push(syn::parse_quote!(__B: ::aethereal_data::Boundary));

    over
}

/// The generics with `bound` required of every field's type, each spanned at that type.
fn with_field_bounds(generics: &Generics, field_types: &[&Type], bound: &TokenStream2) -> Generics {
    let mut bounded = generics.clone();
    let predicates = &mut bounded.make_where_clause().predicates;

    for field_type in field_types {
        let span = field_type.span();
        let spanned_bound: TokenStream2 = bound.clone().into_iter().map(|token| respan(token, span)).collect();
        predicates.push(syn::parse_quote_spanned!(span => #field_type: #spanned_bound));
    }

    bounded
}

/// `token` placed at `span`, so an unmet bound is reported at the field's type.
fn respan(mut token: TokenTree, span: Span) -> TokenTree {
    token.set_span(span);

    token
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
        <#field_type as ::aethereal_data::Encode<__B>>::encode(#value, out)?;
    }
}

/// Decodes into `decode_ident`, spanned at the field's type.
pub fn decode_binding(decode_ident: &Ident, field_type: &Type) -> TokenStream2 {
    let span = field_type.span();
    quote_spanned! { span =>
        let #decode_ident = <#field_type as ::aethereal_data::Decode<__B>>::decode(input)?;
    }
}
