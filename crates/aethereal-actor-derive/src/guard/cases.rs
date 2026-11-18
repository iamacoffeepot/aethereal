//! The cases of an enum of senders: each a name and the one `Sent` type it holds (ADR-0058).

use super::Derive;
use syn::{Data, DeriveInput, Fields, Ident, Type};

/// An enum of senders, read.
pub struct Cases {
    pub name: Ident,
    /// Each case, in the order written, which is the order they are tried.
    pub cases: Vec<Case>,
}

/// One case: its name, and the `Sent` type it holds.
pub struct Case {
    pub name: Ident,
    pub sent: Type,
}

impl Cases {
    /// The cases of `input`, refusing what is not an enum without parameters whose every case holds one unnamed field.
    pub fn read(input: &DeriveInput, derive: Derive) -> syn::Result<Self> {
        let derived = derive.name();
        let Data::Enum(data) = &input.data else {
            let message = format!("`{derived}` is derived for an enum of senders, each case holding one `Sent` type");
            return Err(syn::Error::new_spanned(&input.ident, message));
        };
        let has_parameters = !input.generics.params.is_empty();
        if has_parameters {
            let message = format!("a derived `{derived}` names its senders' types, so it takes no parameters");
            return Err(syn::Error::new_spanned(&input.generics, message));
        }
        if data.variants.is_empty() {
            let message = format!("a derived `{derived}` has a case for each type of sender it admits");
            return Err(syn::Error::new_spanned(&input.ident, message));
        }
        let cases = data.variants.iter().map(|variant| {
            let Fields::Unnamed(fields) = &variant.fields else {
                return Err(one_field(&variant.ident));
            };
            let mut fields = fields.unnamed.iter();
            let (Some(field), None) = (fields.next(), fields.next()) else {
                return Err(one_field(&variant.ident));
            };

            Ok(Case { name: variant.ident.clone(), sent: field.ty.clone() })
        });

        Ok(Self { name: input.ident.clone(), cases: cases.collect::<syn::Result<Vec<Case>>>()? })
    }
}

/// The refusal of the case `name`, which does not hold exactly one unnamed field.
fn one_field(name: &Ident) -> syn::Error {
    syn::Error::new_spanned(
        name,
        "each case holds one field, the `Sent` type of the sender it admits: `Case(ActorRef<T>)`",
    )
}
