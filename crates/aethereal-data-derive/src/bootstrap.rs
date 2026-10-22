//! `#[derive(Bootstrap)]`: an actor whose config is its settings bootstraps itself from them on every host (ADR-0029).

use crate::plain::kind::Declaration;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::DeriveInput;

/// `impl<H: Host> Bootstrap<H>` for the actor `input`: its settings are its config, under the section its kind name's
/// last segment names; it needs nothing, provides nothing, and begins as a singleton from its settings.
pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let Declaration::Named(literal) = Declaration::of(input)? else {
        let message = "`Bootstrap` names the section by the kind's last segment: write `#[kind(name = \"…\")]`";
        return Err(syn::Error::new_spanned(&input.ident, message));
    };
    let has_parameters = !input.generics.params.is_empty();
    if has_parameters {
        return Err(syn::Error::new_spanned(&input.generics, "a derived `Bootstrap` is for a type with no parameters"));
    }

    let name = &input.ident;
    let kind = literal.value();
    let section = last_segment(&kind);

    Ok(quote! {
        #[automatically_derived]
        impl<__H: ::aethereal_substrate::Host> ::aethereal_substrate::Bootstrap<__H> for #name {
            const SECTION: &'static str = #section;
            type Settings = <Self as ::aethereal_substrate::__derive::Actor>::Config;
            type Needs = ();
            type Provides = ();

            fn bootstrap(
                settings: Self::Settings,
                (): (),
                cx: &mut ::aethereal_substrate::Bootstrapping<'_, __H, Self>,
            ) -> ::core::result::Result<(), ::aethereal_substrate::Unbuilt> {
                cx.begin(&(), settings)
            }
        }
    })
}

/// The last segment of the kind name `kind`: what follows its last `.`, or the whole name when it has none.
fn last_segment(kind: &str) -> &str {
    kind.rsplit('.').next().unwrap_or(kind)
}

#[cfg(test)]
mod tests {
    use super::last_segment;

    #[test]
    fn the_section_is_the_last_segment() {
        // Catches a section named by the first segment, or by the whole name with its dots, which `--set` reads as a
        // path's separator.
        assert_eq!(last_segment("aether.example.scene"), "scene");
        assert_eq!(last_segment("scene"), "scene");
    }
}
