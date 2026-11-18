//! What an enum of senders writes: its `Sent` and `SentBy`, and for a guard its `Guard` and `Checks` (ADR-0058).

use super::cases::{Case, Cases};
use proc_macro2::TokenStream;
use quote::quote;

/// `Sent`, whose types are each case's and whose sender is the first case that takes it, and `SentBy`, which holds
/// for a sender of any case's type, found at the case's place.
pub fn sent(cases: &Cases) -> TokenStream {
    let Cases { name, cases } = cases;
    let types = cases.iter().map(|Case { sent, .. }| sent);
    let tuple = cases.iter().map(|Case { sent, .. }| sent);
    let tried = cases.iter().map(|Case { name, sent }| {
        quote! {
            if let ::core::option::Option::Some(sent) = <#sent as ::aethereal_actor::Sent>::from_sender(sender) {
                return ::core::option::Option::Some(Self::#name(sent));
            }
        }
    });

    quote! {
        #[automatically_derived]
        impl ::aethereal_actor::Sent for #name {
            const TYPES: ::aethereal_actor::SenderTypes =
                ::aethereal_actor::SenderTypes::AnyOf(&[#(<#types as ::aethereal_actor::Sent>::TYPES),*]);

            fn from_sender(sender: &::aethereal_actor::Sender<'_>) -> ::core::option::Option<Self> {
                #(#tried)*

                ::core::option::Option::None
            }
        }

        #[automatically_derived]
        impl<__Me, __I> ::aethereal_actor::SentBy<__Me, __I> for #name
        where
            (#(#tuple,)*): ::aethereal_actor::guard::CasesSentBy<__Me, __I>,
        {
        }
    }
}

/// `Guard` for every actor, handing on the enum, and its `Checks` for every kind, through its `Sent`.
pub fn guard(cases: &Cases) -> TokenStream {
    let name = &cases.name;

    quote! {
        #[automatically_derived]
        impl<__A> ::aethereal_actor::Guard<__A> for #name {
            type Sent = Self;
        }

        #[automatically_derived]
        impl<__A, __K> ::aethereal_actor::Checks<__A, __K> for #name {
            fn admits(
                _: &__A,
                _: &__K,
                sender: &::aethereal_actor::Sender<'_>,
            ) -> ::core::option::Option<Self> {
                <Self as ::aethereal_actor::Sent>::from_sender(sender)
            }
        }
    }
}
