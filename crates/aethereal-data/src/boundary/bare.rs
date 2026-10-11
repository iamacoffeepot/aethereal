//! `Bare`: the boundary that offers nothing (ADR-0004).

use crate::Budget;
use crate::boundary::{Boundary, Reader, Writer};
use alloc::vec::Vec;

/// The boundary that offers nothing: a buffer to write, and bytes and a budget to read (ADR-0004).
///
/// What crosses it is plain data. A value that needs an offer from its
/// boundary does not cross `Bare`, and neither does a type that holds one:
///
/// ```compile_fail
/// use aethereal_data::{Bare, Boundary, Encode, EncodeError};
///
/// trait Attach {
///     fn attach(&mut self, payload: &[u8]) -> u32;
/// }
///
/// struct Attached(Vec<u8>);
///
/// impl<B: Boundary> Encode<B> for Attached
/// where
///     for<'a> B::Writer<'a>: Attach,
/// {
///     fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
///         let handle = out.attach(&self.0);
///         Encode::<B>::encode(&handle, out)
///     }
/// }
///
/// let held = (7u8, Attached(Vec::new()));
/// let mut out = Vec::new();
/// let written = Encode::<Bare>::encode(&held, &mut out);
/// ```
pub struct Bare;

impl Boundary for Bare {
    type Writer<'a> = Vec<u8>;
    type Reader<'a> = BareReader<'a>;
}

impl Writer for Vec<u8> {
    fn bytes(&mut self) -> &mut Self {
        self
    }
}

/// The bytes still to read and the budget they are read under, and nothing else.
pub struct BareReader<'input> {
    input: &'input [u8],
    budget: &'input mut Budget,
}

impl<'input> BareReader<'input> {
    /// A reader at the front of `input` that charges `budget`.
    #[must_use]
    pub const fn new(input: &'input [u8], budget: &'input mut Budget) -> Self {
        Self { input, budget }
    }
}

impl<'input> Reader<'input> for BareReader<'input> {
    fn input(&mut self) -> &mut &'input [u8] {
        &mut self.input
    }

    fn budget(&mut self) -> &mut Budget {
        self.budget
    }
}
