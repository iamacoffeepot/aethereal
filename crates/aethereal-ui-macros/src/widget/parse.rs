//! `widget!`'s input read from tokens: `struct Name { param: Type, … }`, then `raises name, …;` when it raises any,
//! then the root of its markup.

use super::Composite;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, Token};

impl Parse for Composite {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let item = input.parse()?;
        let fork = input.fork();
        let raising = fork.parse::<Ident>().is_ok_and(|word| word == "raises") && fork.peek(Ident);
        let raises = if raising {
            input.parse::<Ident>()?;
            let names = Punctuated::<Ident, Token![,]>::parse_separated_nonempty(input)?;
            input.parse::<Token![;]>()?;
            names.into_iter().collect()
        } else {
            Vec::new()
        };
        let root = input.parse()?;
        let more = !input.is_empty();
        if more {
            return Err(input.error("a composite has one root: its markup's other elements stand within it"));
        }

        Ok(Self { item, raises, root })
    }
}
