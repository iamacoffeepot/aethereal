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

    /// `impl Kind` for a named type, bounded as its `Schema` is, and for a type with no parameters a constant that
    /// reads the name so a name that does not conform stops the build though nothing uses the kind.
    pub fn implementation(&self, input: &DeriveInput, schema_generics: &Generics) -> TokenStream2 {
        let Self::Named(literal) = self else {
            return TokenStream2::new();
        };
        let name = &input.ident;
        let (_, ty_generics, _) = input.generics.split_for_impl();
        let (kind_impl, _, kind_where) = schema_generics.split_for_impl();
        let kind_name = quote_spanned! { literal.span() => ::aethereal_data::KindName::new(#literal) };
        let has_parameters = !input.generics.params.is_empty();
        let checked = if has_parameters {
            TokenStream2::new()
        } else {
            quote_spanned! { literal.span() =>
                const _: ::aethereal_data::KindName = <#name as ::aethereal_data::Kind>::NAME;
            }
        };

        quote! {
            #[automatically_derived]
            impl #kind_impl ::aethereal_data::Kind for #name #ty_generics #kind_where {
                const NAME: ::aethereal_data::KindName = #kind_name;
            }

            #checked
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
