//! `style!`'s input read from tokens: each style its attributes, its visibility, its name, the style it starts from
//! after a `:`, and its properties in braces, `name: value`, separated by commas.

use super::{Base, Definition, Property};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Path, Token, braced};

/// Every style of a `style!`.
pub fn definitions(input: ParseStream<'_>) -> syn::Result<Vec<Definition>> {
    let mut definitions = Vec::new();
    while !input.is_empty() {
        definitions.push(input.parse()?);
    }

    Ok(definitions)
}

impl Parse for Definition {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let visibility = input.parse()?;
        let name = input.parse()?;
        let based = input.peek(Token![:]);
        let base = if based {
            input.parse::<Token![:]>()?;
            Base::Style(input.call(Path::parse_mod_style)?)
        } else {
            Base::Default
        };
        let content;
        braced!(content in input);
        let properties = Punctuated::<Property, Token![,]>::parse_terminated(&content)?;

        Ok(Self { attributes, visibility, name, base, properties: properties.into_iter().collect() })
    }
}

impl Parse for Property {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![:]>()?;

        Ok(Self { name, value: input.parse()? })
    }
}
