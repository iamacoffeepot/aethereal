//! The macros of `aethereal-actor` (ADR-0058, ADR-0059), which the actor crate re-exports, so an actor is written with
//! `aethereal_actor::actor`:
//!
//! ```text
//! #[actor(key, parent, depends, spawns, with)] impl Name { begin, end, handlers, methods }
//!     ──> ChildOf, Actor with its Handles collected from the handlers, one Handles<K> or Receives<H> for each handler,
//!         and an impl Name of the rest
//! #[protocol(Kind, Kind(answers = A), …)] struct P ──> Protocol, and one Carries<K> for each kind
//! #[derive(Guard)] enum G { Case(SentType), … } ──> Guard, Checks, Sent, and SentBy
//! #[derive(Sent)] enum S { Case(SentType), … } ──> Sent and SentBy
//! ```
//!
//! Each reads its input, checks what it can see, and refuses what it cannot write with a `compile_error!` at the
//! offending token; what only the types can say is left to the trait impls it writes. The actor crate's docs show each
//! in use, and its doctests hold what each refuses.

mod actor;
mod guard;
mod protocol;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// An actor written as one block (ADR-0059): `#[actor(…)]` on `impl Name { … }` writes `ChildOf`, `Actor`, and one
/// `Handles<K>` or `Receives<H>` for each method that carries a role word. Use it through `aethereal_actor::actor`.
#[proc_macro_attribute]
pub fn actor(attribute: TokenStream, item: TokenStream) -> TokenStream {
    actor::expand(attribute.into(), item.into()).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// A named protocol (ADR-0016, ADR-0059): `#[protocol(…)]` on a struct writes its `Protocol` and one `Carries` for each
/// kind it lists. Use it through `aethereal_actor::protocol`.
#[proc_macro_attribute]
pub fn protocol(attribute: TokenStream, item: TokenStream) -> TokenStream {
    protocol::expand(attribute.into(), item.into()).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// A guard of several senders, an enum of them (ADR-0058): `Guard` for every actor, its `Checks` for every kind, and
/// what `#[derive(Sent)]` writes. Use it through `aethereal_actor::Guard`.
#[proc_macro_derive(Guard)]
pub fn derive_guard(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    guard::expand(&input, guard::Derive::Guard).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// A checked sender of several types, an enum of them (ADR-0058): `Sent`, its cases tried in the order written, and
/// `SentBy`, which holds for the type of each case. Use it through `aethereal_actor::Sent`.
#[proc_macro_derive(Sent)]
pub fn derive_sent(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    guard::expand(&input, guard::Derive::Sent).unwrap_or_else(syn::Error::into_compile_error).into()
}
