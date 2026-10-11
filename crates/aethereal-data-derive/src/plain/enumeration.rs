//! An enum's `Schema` and `Plain` impls with selectors (ADR-0004).

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use super::{decode_binding, encode_call, shape_field, with_plain_bounds};
use syn::{Attribute, DataEnum, DeriveInput, FieldsNamed, FieldsUnnamed, LitInt, Token, Variant};

/// One variant's three emitted pieces.
struct Built {
    shape: TokenStream2,
    encode_arm: TokenStream2,
    decode_arm: TokenStream2,
}

/// Expands an enum into `Schema` and `Plain` impls.
pub fn expand(input: &DeriveInput, data: &DataEnum) -> syn::Result<TokenStream2> {
    let is_empty = data.variants.is_empty();
    if is_empty {
        let message = "Plain derive does not support enums with no variants";
        return Err(syn::Error::new_spanned(&input.ident, message));
    }

    let mut shapes = Vec::new();
    let mut encodes = Vec::new();
    let mut decodes = Vec::new();
    let mut used: Vec<u32> = Vec::new();

    for (position, variant) in data.variants.iter().enumerate() {
        let position_selector = u32::try_from(position)
            .map_err(|_| syn::Error::new_spanned(&variant.ident, "too many variants for a u32 selector"))?;
        let selector = selector_of(variant, position_selector)?;
        let already_used = used.contains(&selector);
        if already_used {
            let second = variant.ident.to_string();
            let message = format!("duplicate selector {selector} for variant {second}");
            return Err(syn::Error::new_spanned(&variant.ident, message));
        }
        used.push(selector);

        let built = build_variant(variant, selector)?;
        shapes.push(built.shape);
        encodes.push(built.encode_arm);
        decodes.push(built.decode_arm);
    }

    Ok(finish(input, &shapes, &encodes, &decodes))
}

/// Assembles the two impls from their pieces.
fn finish(
    input: &DeriveInput,
    shapes: &[TokenStream2],
    encodes: &[TokenStream2],
    decodes: &[TokenStream2],
) -> TokenStream2 {
    let name = &input.ident;
    let type_name = name.to_string();
    let generics = with_plain_bounds(&input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics ::aethereal_data::Schema for #name #ty_generics #where_clause {
            const SHAPE: ::aethereal_data::Shape = ::aethereal_data::Shape::Enum {
                name: #type_name,
                variants: &[ #( #shapes ),* ],
            };
        }

        impl #impl_generics ::aethereal_data::Plain for #name #ty_generics #where_clause {
            fn encode(&self, out: &mut ::aethereal_data::__derive::Vec<u8>) -> ::core::result::Result<(), ::aethereal_data::EncodeError> {
                match self {
                    #( #encodes, )*
                }
                ::core::result::Result::Ok(())
            }

            fn decode(
                input: &mut &[u8],
                budget: &mut ::aethereal_data::Budget,
            ) -> ::core::result::Result<Self, ::aethereal_data::DecodeError> {
                let selector = ::aethereal_data::__derive::read_selector(input)?;
                match selector {
                    #( #decodes, )*
                    other => ::core::result::Result::Err(
                        ::aethereal_data::DecodeError::InvalidSelector { name: #type_name, selector: other },
                    ),
                }
            }
        }
    }
}

/// The variant's selector: its position unless `#[plain(selector = N)]` says otherwise.
fn selector_of(variant: &Variant, position: u32) -> syn::Result<u32> {
    let mut selector: Option<u32> = None;

    for attr in &variant.attrs {
        let is_plain = attr.path().is_ident("plain");
        if is_plain {
            let found = parse_selector_attr(attr)?;
            let already = selector.is_some();
            if already {
                let name = variant.ident.to_string();
                let message = format!("repeated selector for variant {name}");
                return Err(syn::Error::new_spanned(attr, message));
            }
            selector = Some(found);
        }
    }

    let chosen = selector.unwrap_or(position);

    Ok(chosen)
}

/// Parses `#[plain(selector = N)]` into its `u32`.
fn parse_selector_attr(attr: &Attribute) -> syn::Result<u32> {
    let parsed = attr.parse_args_with(|input: syn::parse::ParseStream| {
        let key: syn::Ident = input.parse()?;
        let is_selector = key == "selector";
        if !is_selector {
            let message = format!("expected selector = N, found {key}");
            return Err(syn::Error::new_spanned(key, message));
        }
        let _equals: Token![=] = input.parse()?;
        let literal: LitInt = input.parse()?;
        let done = input.is_empty();
        if !done {
            let message = "expected selector = N with nothing after it";
            return Err(syn::Error::new_spanned(literal, message));
        }
        literal.base10_parse::<u32>()
    })?;

    Ok(parsed)
}

/// Builds one variant's shape and match arms.
fn build_variant(variant: &Variant, selector: u32) -> syn::Result<Built> {
    match &variant.fields {
        syn::Fields::Named(fields) => build_named(variant, selector, fields),
        syn::Fields::Unnamed(fields) => Ok(build_unnamed(variant, selector, fields)),
        syn::Fields::Unit => Ok(build_unit(variant, selector)),
    }
}

/// A struct variant's pieces, saying what was written.
fn build_named(variant: &Variant, selector: u32, fields: &FieldsNamed) -> syn::Result<Built> {
    let name = &variant.ident;
    let variant_name = name.to_string();
    let mut shape_fields = Vec::new();
    let mut encode_statements = Vec::new();
    let mut decode_bindings = Vec::new();
    let mut build_fields = Vec::new();
    let mut pattern_fields = Vec::new();

    for (position, field) in fields.named.iter().enumerate() {
        let Some(field_ident) = &field.ident else {
            let message = "Plain derive expects named fields to have names";
            return Err(syn::Error::new_spanned(field, message));
        };
        let field_type = &field.ty;
        let field_name = field_ident.to_string();
        let decode_ident = format_ident!("__field{position}");
        let shape = shape_field(&field_name, field_type);
        let access = quote! { #field_ident };
        let encode = encode_call(&access, field_type);
        let binding = decode_binding(&decode_ident, field_type);
        let build = quote! { #field_ident: #decode_ident };

        shape_fields.push(shape);
        encode_statements.push(encode);
        decode_bindings.push(binding);
        build_fields.push(build);
        pattern_fields.push(field_ident);
    }

    let shape = quote! {
        ::aethereal_data::Variant {
            name: #variant_name,
            selector: #selector,
            fields: ::aethereal_data::Fields::Named(&[ #( #shape_fields ),* ]),
        }
    };
    let encode_arm = quote! {
        Self::#name { #( #pattern_fields ),* } => {
            ::aethereal_data::__derive::write_selector(out, #selector);
            #( #encode_statements )*
        }
    };
    let decode_arm = quote! {
        #selector => {
            #( #decode_bindings )*
            ::core::result::Result::Ok(Self::#name { #( #build_fields ),* })
        }
    };
    let built = Built { shape, encode_arm, decode_arm };

    Ok(built)
}

/// A tuple variant's pieces; an empty one is a bare selector like a unit variant.
fn build_unnamed(variant: &Variant, selector: u32, fields: &FieldsUnnamed) -> Built {
    let name = &variant.ident;
    let variant_name = name.to_string();
    let is_empty = fields.unnamed.is_empty();
    if is_empty {
        let shape = quote! {
            ::aethereal_data::Variant {
                name: #variant_name,
                selector: #selector,
                fields: ::aethereal_data::Fields::Unnamed(&[]),
            }
        };
        let encode_arm = quote! {
            Self::#name () => {
                ::aethereal_data::__derive::write_selector(out, #selector);
            }
        };
        let decode_arm = quote! {
            #selector => ::core::result::Result::Ok(Self::#name ())
        };
        let built = Built { shape, encode_arm, decode_arm };

        return built;
    }

    let mut shapes = Vec::new();
    let mut encodes = Vec::new();
    let mut decodes = Vec::new();
    let mut encode_idents = Vec::new();
    let mut build_idents = Vec::new();

    for (position, field) in fields.unnamed.iter().enumerate() {
        let field_type = &field.ty;
        let encode_ident = format_ident!("__field{position}");
        let decode_ident = format_ident!("__field{position}");
        let shape = quote! { <#field_type as ::aethereal_data::Schema>::SHAPE };
        let access = quote! { #encode_ident };
        let encode = encode_call(&access, field_type);
        let binding = decode_binding(&decode_ident, field_type);

        shapes.push(shape);
        encodes.push(encode);
        decodes.push(binding);
        encode_idents.push(encode_ident);
        build_idents.push(decode_ident);
    }

    // Encode and decode share positional names in separate scopes, so clone one
    // list for the pattern rather than minting a second set of names.
    let pattern_idents = encode_idents.clone();
    let shape = quote! {
        ::aethereal_data::Variant {
            name: #variant_name,
            selector: #selector,
            fields: ::aethereal_data::Fields::Unnamed(&[ #( #shapes ),* ]),
        }
    };
    let encode_arm = quote! {
        Self::#name (#( #pattern_idents ),*) => {
            ::aethereal_data::__derive::write_selector(out, #selector);
            #( #encodes )*
        }
    };
    let decode_arm = quote! {
        #selector => {
            #( #decodes )*
            ::core::result::Result::Ok(Self::#name (#( #build_idents ),*))
        }
    };
    Built { shape, encode_arm, decode_arm }
}

/// A unit variant's pieces: the selector alone.
fn build_unit(variant: &Variant, selector: u32) -> Built {
    let name = &variant.ident;
    let variant_name = name.to_string();
    let shape = quote! {
        ::aethereal_data::Variant {
            name: #variant_name,
            selector: #selector,
            fields: ::aethereal_data::Fields::Unnamed(&[]),
        }
    };
    let encode_arm = quote! {
        Self::#name => {
            ::aethereal_data::__derive::write_selector(out, #selector);
        }
    };
    let decode_arm = quote! {
        #selector => ::core::result::Result::Ok(Self::#name)
    };

    Built { shape, encode_arm, decode_arm }
}
