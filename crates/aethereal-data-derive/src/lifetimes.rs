//! Refusing lifetime parameters, shared by the derives (ADR-0004).

use syn::{GenericParam, Generics};

/// Refuses a lifetime parameter for `derive`; decode owns its data.
pub fn reject_lifetimes(generics: &Generics, derive: &str) -> syn::Result<()> {
    for param in &generics.params {
        if let GenericParam::Lifetime(lifetime) = param {
            let message = format!("{derive} derive does not support lifetime parameters; decode owns its data");
            return Err(syn::Error::new_spanned(lifetime, message));
        }
    }

    Ok(())
}
