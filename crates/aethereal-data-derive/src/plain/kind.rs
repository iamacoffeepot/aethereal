//! `#[kind(name = "…")]`: a derived type named as a kind (ADR-0006).

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Attribute, Data, DeriveInput, Expr, ExprLit, Generics, Lit, LitStr};

/// Whether the derived type is a kind, read from its attributes.
pub enum Declaration {
    /// `#[kind(name = …)]` names it, with this literal.
    Named(LitStr),
    /// No `#[kind]` is written: the derive implements no `Kind`.
    Unnamed,
}

impl Declaration {
    /// The declaration `input` writes, refusing a `#[kind]` that is repeated, misplaced, or holds anything but a name.
    pub fn of(input: &DeriveInput) -> syn::Result<Self> {
        refuse_inner(input)?;
        let mut declaration = Self::Unnamed;

        for attr in input.attrs.iter().filter(|attr| is_kind(attr)) {
            let already = matches!(declaration, Self::Named(_));
            if already {
                return Err(syn::Error::new_spanned(attr, "`kind` is written once on a type"));
            }
            declaration = Self::Named(name_of(attr)?);
        }

        Ok(declaration)
    }

    /// `impl Kind` for a named type. A type with parameters is bounded as its `Schema` is, and its node is made in a
    /// constant. A type with no parameters gets a static node, and a constant that computes its id, so a name that does
    /// not conform, a cycle through a generic kind, two members that tie, or a graph past a cap stops the build where
    /// the kind is defined, though nothing uses it (ADR-0006).
    pub fn implementation(&self, input: &DeriveInput, schema_generics: &Generics) -> TokenStream2 {
        let Self::Named(literal) = self else {
            return TokenStream2::new();
        };
        let name = &input.ident;
        let (_, ty_generics, _) = input.generics.split_for_impl();
        let (kind_impl, _, kind_where) = schema_generics.split_for_impl();
        let kind_name = quote_spanned! { literal.span() => ::aethereal_data::KindName::new(#literal) };
        let has_parameters = !input.generics.params.is_empty();
        if has_parameters {
            return quote! {
                #[automatically_derived]
                impl #kind_impl ::aethereal_data::Kind for #name #ty_generics #kind_where {
                    const NAME: ::aethereal_data::KindName = #kind_name;
                }
            };
        }

        // A non-generic kind's node is a static, so a cycle of kinds reaches it before it is evaluated: its site, the
        // place it is defined, tells it from every other kind's node, as a constant cannot compare pointers.
        let site = quote_spanned! { name.span() =>
            ::core::concat!(
                ::core::module_path!(), "::", ::core::stringify!(#name), "@",
                ::core::file!(), ":", ::core::line!(), ":", ::core::column!()
            )
        };
        let id = quote_spanned! { name.span() =>
            const _: ::aethereal_data::KindId = <#name as ::aethereal_data::Kind>::KIND.id();
        };

        // `KindPtr::defined` is sound here: `NODE` is an immutable static, which lives for the whole program and is
        // never written, and `&raw const` takes its address without reading it, so a cycle of kinds may point at it
        // before it is evaluated. Nothing else names `NODE`.
        quote! {
            const _: () = {
                static NODE: ::aethereal_data::KindShape = ::aethereal_data::KindShape {
                    name: <#name as ::aethereal_data::Kind>::NAME,
                    site: ::aethereal_data::Site::Defined { at: #site },
                    shape: &<#name as ::aethereal_data::Schema>::SHAPE,
                };

                #[automatically_derived]
                impl ::aethereal_data::Kind for #name {
                    const NAME: ::aethereal_data::KindName = #kind_name;
                    const KIND: ::aethereal_data::KindPtr = unsafe { ::aethereal_data::KindPtr::defined(&raw const NODE) };
                }
            };

            #id
        }
    }
}

/// Whether `attr` is `#[kind …]`.
fn is_kind(attr: &Attribute) -> bool {
    attr.path().is_ident("kind")
}

/// The name literal of one `#[kind(name = "…")]`, refusing any other key, a value that is not a string literal, and
/// a missing name.
fn name_of(attr: &Attribute) -> syn::Result<LitStr> {
    let mut found: Vec<LitStr> = Vec::new();

    attr.parse_nested_meta(|meta| {
        let is_name = meta.path.is_ident("name");
        if !is_name {
            let message = "`kind` holds `name = \"…\"` and nothing else";
            return Err(meta.error(message));
        }
        let already = !found.is_empty();
        if already {
            return Err(meta.error("`name` is written once"));
        }
        let mut value: Expr = meta.value()?.parse()?;
        // A `macro_rules!` fragment such as `$name:literal` reaches the derive wrapped in invisible groups.
        while let Expr::Group(group) = value {
            value = *group.expr;
        }
        let Expr::Lit(ExprLit { lit: Lit::Str(literal), .. }) = value else {
            return Err(syn::Error::new_spanned(value, "a kind's name is a string literal"));
        };
        found.push(literal);

        Ok(())
    })?;

    found.pop().ok_or_else(|| syn::Error::new_spanned(attr, "`kind` needs its name: `#[kind(name = \"…\")]`"))
}

/// Refuses `#[kind]` on a field or a variant; it names the type and is written on the type.
fn refuse_inner(input: &DeriveInput) -> syn::Result<()> {
    let inner: Vec<&Attribute> = match &input.data {
        Data::Struct(data) => data.fields.iter().flat_map(|field| &field.attrs).collect(),
        Data::Enum(data) => {
            let variants = data.variants.iter().flat_map(|variant| &variant.attrs);
            let fields = data.variants.iter().flat_map(|variant| &variant.fields).flat_map(|field| &field.attrs);
            variants.chain(fields).collect()
        }
        Data::Union(_) => Vec::new(),
    };

    for attr in inner {
        if is_kind(attr) {
            return Err(syn::Error::new_spanned(attr, "`kind` names the type and is written on the type"));
        }
    }

    Ok(())
}
