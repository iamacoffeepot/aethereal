//! `#[section(name = "…")]` and `#[flag]`: a config kind named as a section, with a handle for each named field and
//! the flags its part suggests (ADR-0028).

use super::kind::Declaration;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Expr, ExprLit, Field, Fields, Lit, LitChar, LitStr, Meta};

/// Whether the derived type is a section, read from its attributes.
pub enum Sectioned {
    /// `#[section(name = …)]` names it, with this literal.
    Named(LitStr),
    /// No `#[section]` is written: the derive writes no `Section` and no handles.
    Unnamed,
}

impl Sectioned {
    /// The section `input` declares, refusing a `#[section]` that is repeated, misplaced, holds anything but a name,
    /// or names a type that is not a kind, and a `#[flag]` anywhere but on a named field of a section.
    pub fn of(input: &DeriveInput, declaration: &Declaration) -> syn::Result<Self> {
        let mut sectioned = Self::Unnamed;

        for attr in input.attrs.iter().filter(|attr| is_section(attr)) {
            let already = matches!(sectioned, Self::Named(_));
            if already {
                return Err(syn::Error::new_spanned(attr, "`section` is written once on a type"));
            }
            sectioned = Self::Named(name_of(attr)?);
        }

        sectioned.refuse_misplaced(input, declaration)?;

        Ok(sectioned)
    }

    /// Refuses a section that is not a plain struct kind, and `#[section]` or `#[flag]` where they do not belong.
    fn refuse_misplaced(&self, input: &DeriveInput, declaration: &Declaration) -> syn::Result<()> {
        let Data::Struct(data) = &input.data else {
            return refuse_in_enum(input, self);
        };

        for attr in data.fields.iter().flat_map(|field| &field.attrs) {
            if is_section(attr) {
                return Err(syn::Error::new_spanned(attr, "`section` names the type and is written on the type"));
            }
        }

        let Self::Named(literal) = self else {
            let flagged = data.fields.iter().flat_map(|field| &field.attrs).find(|attr| is_flag(attr));
            if let Some(attr) = flagged {
                let message = "`flag` suggests a flag for a field of a `#[section]` struct";
                return Err(syn::Error::new_spanned(attr, message));
            }
            return Ok(());
        };

        let is_kind = matches!(declaration, Declaration::Named(_));
        if !is_kind {
            let message = "a section is a kind: write `#[kind(name = \"…\")]` beside `#[section]`";
            return Err(syn::Error::new_spanned(literal, message));
        }
        let has_parameters = !input.generics.params.is_empty();
        if has_parameters {
            return Err(syn::Error::new_spanned(&input.generics, "a section has no type parameters"));
        }
        let positional = matches!(data.fields, Fields::Unnamed(_));
        if positional {
            let flagged = data.fields.iter().flat_map(|field| &field.attrs).find(|attr| is_flag(attr));
            if let Some(attr) = flagged {
                return Err(syn::Error::new_spanned(attr, "`flag` is written on a named field"));
            }
        }

        Ok(())
    }

    /// `impl Section` and the field handles for a section; nothing otherwise.
    pub fn implementation(&self, input: &DeriveInput) -> syn::Result<TokenStream2> {
        let Self::Named(literal) = self else {
            return Ok(TokenStream2::new());
        };
        let Data::Struct(data) = &input.data else {
            return Ok(TokenStream2::new());
        };
        let name = &input.ident;
        let named: Vec<&Field> = match &data.fields {
            Fields::Named(fields) => fields.named.iter().collect(),
            Fields::Unnamed(_) | Fields::Unit => Vec::new(),
        };

        let mut declared = Vec::new();
        let mut handles = Vec::new();
        for field in named {
            let Some(ident) = &field.ident else {
                return Err(syn::Error::new_spanned(field, "the derive expects named fields to have names"));
            };
            let field_name = ident.to_string();
            let field_name = field_name.trim_start_matches("r#");
            let doc = doc_of(&field.attrs);
            let flag = suggested(field, field_name)?;
            declared.push(quote! {
                ::aethereal_data::config::Declared { name: #field_name, doc: #doc, flag: #flag }
            });

            let handle = format_ident!("{}", field_name.to_uppercase());
            let visibility = &field.vis;
            let field_type = &field.ty;
            let handle_doc = format!("The handle of the field `{field_name}`.");
            handles.push(quote! {
                #[doc = #handle_doc]
                #visibility const #handle: ::aethereal_data::config::Field<Self, #field_type> =
                    ::aethereal_data::__derive::field(#field_name);
            });
        }

        Ok(quote! {
            #[automatically_derived]
            impl ::aethereal_data::config::Section for #name {
                const SECTION: &'static str = #literal;
                const FIELDS: &'static [::aethereal_data::config::Declared] = &[ #( #declared ),* ];
            }

            #[automatically_derived]
            #[allow(dead_code)]
            impl #name {
                #( #handles )*
            }
        })
    }
}

/// Refuses `#[section]` and `#[flag]` on an enum: a section is a struct of fields.
fn refuse_in_enum(input: &DeriveInput, sectioned: &Sectioned) -> syn::Result<()> {
    if let Sectioned::Named(literal) = sectioned {
        return Err(syn::Error::new_spanned(literal, "a section is a struct"));
    }
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };
    let variants = data.variants.iter().flat_map(|variant| &variant.attrs);
    let fields = data.variants.iter().flat_map(|variant| &variant.fields).flat_map(|field| &field.attrs);
    for attr in variants.chain(fields) {
        let misplaced = is_section(attr) || is_flag(attr);
        if misplaced {
            return Err(syn::Error::new_spanned(attr, "`section` and `flag` belong on a struct and its fields"));
        }
    }

    Ok(())
}

/// Whether `attr` is `#[section …]`.
fn is_section(attr: &Attribute) -> bool {
    attr.path().is_ident("section")
}

/// Whether `attr` is `#[flag …]`.
fn is_flag(attr: &Attribute) -> bool {
    attr.path().is_ident("flag")
}

/// The name literal of one `#[section(name = "…")]`, refusing any other key and a missing name.
fn name_of(attr: &Attribute) -> syn::Result<LitStr> {
    let mut found: Vec<LitStr> = Vec::new();

    attr.parse_nested_meta(|meta| {
        let is_name = meta.path.is_ident("name");
        if !is_name {
            return Err(meta.error("`section` holds `name = \"…\"` and nothing else"));
        }
        let already = !found.is_empty();
        if already {
            return Err(meta.error("`name` is written once"));
        }
        found.push(string_value(meta.value()?.parse()?)?);

        Ok(())
    })?;

    found.pop().ok_or_else(|| syn::Error::new_spanned(attr, "`section` needs its name: `#[section(name = \"…\")]`"))
}

/// The string literal `value` is, through the invisible groups a `macro_rules!` fragment arrives in.
fn string_value(mut value: Expr) -> syn::Result<LitStr> {
    while let Expr::Group(group) = value {
        value = *group.expr;
    }
    let Expr::Lit(ExprLit { lit: Lit::Str(literal), .. }) = value else {
        return Err(syn::Error::new_spanned(value, "expected a string literal"));
    };

    Ok(literal)
}

/// The char literal `value` is, through the invisible groups a `macro_rules!` fragment arrives in.
fn char_value(mut value: Expr) -> syn::Result<LitChar> {
    while let Expr::Group(group) = value {
        value = *group.expr;
    }
    let Expr::Lit(ExprLit { lit: Lit::Char(literal), .. }) = value else {
        return Err(syn::Error::new_spanned(value, "expected a char literal"));
    };

    Ok(literal)
}

/// The field's doc comment: each `///` line without its first space, joined by newlines.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        let is_doc = attr.path().is_ident("doc");
        if !is_doc {
            continue;
        }
        let Meta::NameValue(pair) = &attr.meta else {
            continue;
        };
        let Expr::Lit(ExprLit { lit: Lit::Str(literal), .. }) = &pair.value else {
            continue;
        };
        let line = literal.value();
        let line = line.strip_prefix(' ').unwrap_or(&line).to_owned();
        lines.push(line);
    }

    lines.join("\n").trim().to_owned()
}

/// The `Suggested` the field's `#[flag]` writes: its `long`, by default the field's name with `_` written `-`, and
/// its `short` when it has one.
fn suggested(field: &Field, field_name: &str) -> syn::Result<TokenStream2> {
    let mut flags = field.attrs.iter().filter(|attr| is_flag(attr));
    let Some(attr) = flags.next() else {
        return Ok(quote! { ::aethereal_data::config::Suggested::NoFlag });
    };
    if let Some(second) = flags.next() {
        return Err(syn::Error::new_spanned(second, "`flag` is written once on a field"));
    }

    let mut long: Vec<LitStr> = Vec::new();
    let mut short: Vec<LitChar> = Vec::new();
    let bare = matches!(attr.meta, Meta::Path(_));
    if !bare {
        attr.parse_nested_meta(|meta| {
            let is_long = meta.path.is_ident("long");
            let is_short = meta.path.is_ident("short");
            if is_long {
                let already = !long.is_empty();
                if already {
                    return Err(meta.error("`long` is written once"));
                }
                long.push(string_value(meta.value()?.parse()?)?);
                return Ok(());
            }
            if is_short {
                let already = !short.is_empty();
                if already {
                    return Err(meta.error("`short` is written once"));
                }
                short.push(char_value(meta.value()?.parse()?)?);
                return Ok(());
            }
            Err(meta.error("`flag` holds `long = \"…\"` and `short = '…'`"))
        })?;
    }

    let long = long.pop().map_or_else(|| field_name.replace('_', "-"), |literal| literal.value());
    let flag = short.pop().map_or_else(
        || quote! { ::aethereal_data::config::Suggested::Long { long: #long } },
        |letter| quote! { ::aethereal_data::config::Suggested::LongAndShort { long: #long, short: #letter } },
    );

    Ok(flag)
}
