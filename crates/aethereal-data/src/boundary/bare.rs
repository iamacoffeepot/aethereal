//! `Bare`: the boundary that offers nothing (ADR-0004).

use crate::Boundary;

/// The boundary that offers nothing beside the bytes and the budget (ADR-0004).
///
/// What crosses it is plain data. A value that needs an offer from its
/// boundary does not cross `Bare`, and neither does a type that holds one:
///
/// ```compile_fail
/// use aethereal_data::{Bare, Boundary, Encode, EncodeError, Writer};
///
/// trait Attach {
///     fn attach(&mut self, payload: &[u8]) -> u32;
/// }
///
/// struct Attached(Vec<u8>);
///
/// impl<B: Boundary> Encode<B> for Attached
/// where
///     for<'a> B::WriterOffers<'a>: Attach,
/// {
///     fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
///         out.offers().attach(&self.0).encode(out)
///     }
/// }
///
/// let held = (7u8, Attached(Vec::new()));
/// let mut out = Writer::<Bare>::new(());
/// let written = held.encode(&mut out);
/// ```
pub struct Bare;

impl Boundary for Bare {
    type WriterOffers<'a> = ();
    type ReaderOffers<'a> = ();
}
