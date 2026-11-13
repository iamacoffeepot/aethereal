//! `view!`'s input read from tokens: `Screen, field, Root { … }`.

use super::View;
use syn::Token;
use syn::parse::{Parse, ParseStream};

impl Parse for View {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let screen = input.parse()?;
        input.parse::<Token![,]>()?;
        let field = input.parse()?;
        input.parse::<Token![,]>()?;
        let root = input.parse()?;
        input.parse::<Option<Token![,]>>()?;
        let more = !input.is_empty();
        if more {
            return Err(input.error("a view has one root: its markup's other elements stand within it"));
        }

        Ok(Self { screen, field, root })
    }
}
