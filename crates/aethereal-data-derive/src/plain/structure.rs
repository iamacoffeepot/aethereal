//! A struct's shape and codec bodies (ADR-0004).

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use super::{Codec, Declaration, Derive, decode_binding, encode_call, impls, shape_field};
use syn::{DeriveInput, FieldsNamed, FieldsUnnamed, Type};

/// The pieces one struct's impls are built from.
struct Parts {
    shape_fields: Vec<TokenStream2>,
    encode_statements: Vec<TokenStream2>,
    decode_bindings: Vec<TokenStream2>,
    construction: TokenStream2,
}

/// Expands a struct into the impls of `derive`.
pub fn expand(
    input: &DeriveInput,
    data: &syn::DataStruct,
    derive: Derive,
    declaration: &Declaration,
) -> syn::Result<TokenStream2> {
    let parts = match &data.fields {
        syn::Fields::Named(fields) => named_parts(fields)?,
        syn::Fields::Unnamed(fields) => unnamed_parts(fields),
        syn::Fields::Unit => unit_parts(),
    };
    let field_types: Vec<&Type> = data.fields.iter().map(|field| &field.ty).collect();

    Ok(impls(input, &field_types, &codec(input, &parts), derive, declaration))
}

/// The struct's shape and the bodies of its codec.
fn codec(input: &DeriveInput, parts: &Parts) -> Codec {
    let type_name = input.ident.to_string();
    let shape_fields = &parts.shape_fields;
    let encode_statements = &parts.encode_statements;
    let decode_bindings = &parts.decode_bindings;
    let construction = &parts.construction;
    let shape = quote! {
        ::aethereal_data::Shape::Struct {
            name: #type_name,
            fields: &[ #( #shape_fields ),* ],
        }
    };
    let encode = quote! {
        #( #encode_statements )*
        ::core::result::Result::Ok(())
    };
    let decode = quote! {
        #( #decode_bindings )*
        ::core::result::Result::Ok(#construction)
    };

    Codec { shape, encode, decode }
}

/// Pieces for a struct with named fields.
fn named_parts(fields: &FieldsNamed) -> syn::Result<Parts> {
    let mut shape_fields = Vec::new();
    let mut encode_statements = Vec::new();
    let mut decode_bindings = Vec::new();
    let mut build_fields = Vec::new();

    for (position, field) in fields.named.iter().enumerate() {
        let Some(field_ident) = &field.ident else {
            let message = "the derive expects named fields to have names";
            return Err(syn::Error::new_spanned(field, message));
        };
        let field_type = &field.ty;
        let field_name = field_ident.to_string();
        let decode_ident = format_ident!("__field{position}");
        let shape = shape_field(&field_name, field_type);
        let access = quote! { &self.#field_ident };
        let encode = encode_call(&access, field_type);
        let binding = decode_binding(&decode_ident, field_type);
        let build = quote! { #field_ident: #decode_ident };

        shape_fields.push(shape);
        encode_statements.push(encode);
        decode_bindings.push(binding);
        build_fields.push(build);
    }

    let construction = quote! { Self { #( #build_fields ),* } };
    let parts = Parts { shape_fields, encode_statements, decode_bindings, construction };

    Ok(parts)
}

/// Pieces for a struct with unnamed fields.
fn unnamed_parts(fields: &FieldsUnnamed) -> Parts {
    let mut shape_fields = Vec::new();
    let mut encode_statements = Vec::new();
    let mut decode_bindings = Vec::new();
    let mut build_idents = Vec::new();

    for (position, field) in fields.unnamed.iter().enumerate() {
        let field_type = &field.ty;
        let position_name = position.to_string();
        let index = syn::Index::from(position);
        let decode_ident = format_ident!("__field{position}");
        let shape = shape_field(&position_name, field_type);
        let access = quote! { &self.#index };
        let encode = encode_call(&access, field_type);
        let binding = decode_binding(&decode_ident, field_type);

        shape_fields.push(shape);
        encode_statements.push(encode);
        decode_bindings.push(binding);
        build_idents.push(decode_ident);
    }

    let construction = quote! { Self(#( #build_idents ),*) };
    Parts { shape_fields, encode_statements, decode_bindings, construction }
}

/// Pieces for a unit struct: an empty shape and no bytes.
fn unit_parts() -> Parts {
    let construction = quote! { Self };
    Parts { shape_fields: Vec::new(), encode_statements: Vec::new(), decode_bindings: Vec::new(), construction }
}
