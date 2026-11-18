//! `#[derive(Guard)]` and `#[derive(Sent)]`: a guard of senders of several types, a named enum of them, each case
//! holding one `Sent` type (ADR-0058).
//!
//! ```text
//! enum Peer { Exchange(ActorRef<Exchange>), Client(ActorRef<Client>) }
//!     ──> Sent: TYPES, AnyOf each case's; from_sender, each case's tried in the order written
//!     ──> SentBy<Me, I>: when `Me` sends as one of the cases' types, found at `I`
//!     ──> (Guard only) Guard<A> for every actor, its Sent itself; Checks<A, K> for every kind, through from_sender
//! ```

mod cases;
mod emit;

use cases::Cases;
use proc_macro2::TokenStream;
use syn::DeriveInput;

/// Which derive is written.
#[derive(Clone, Copy)]
pub enum Derive {
    /// A guard: what `Sent` writes, and the guard itself.
    Guard,
    /// A checked sender alone, for a guard of a handler's own to hand on.
    Sent,
}

/// The impls `derive` writes for the enum `input`.
pub fn expand(input: &DeriveInput, derive: Derive) -> syn::Result<TokenStream> {
    let cases = Cases::read(input, derive)?;
    let sent = emit::sent(&cases);
    let guard = match derive {
        Derive::Guard => emit::guard(&cases),
        Derive::Sent => TokenStream::new(),
    };

    Ok(quote::quote! {
        #sent
        #guard
    })
}

impl Derive {
    /// Its name, as written in `#[derive(…)]`.
    const fn name(self) -> &'static str {
        match self {
            Self::Guard => "Guard",
            Self::Sent => "Sent",
        }
    }
}
