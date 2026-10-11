//! `Reader`: what a decode at a boundary reads from (ADR-0004).

use crate::{Boundary, Budget};

/// The bytes still to read, the allocation limit, and what the boundary `B` offers beside them (ADR-0004).
///
/// There is one reader type, over the boundary. A codec advances
/// [`input`](Self::input) and charges [`budget`](Self::budget); a bound value
/// also calls what it needs of [`offers`](Self::offers).
pub struct Reader<'a, B: Boundary> {
    input: &'a [u8],
    budget: &'a mut Budget,
    offers: B::ReaderOffers<'a>,
}

impl<'a, B: Boundary> Reader<'a, B> {
    /// A reader at the front of `input` that charges `budget`, holding what `B` offers.
    #[must_use]
    pub const fn new(input: &'a [u8], budget: &'a mut Budget, offers: B::ReaderOffers<'a>) -> Self {
        Self { input, budget, offers }
    }

    /// The bytes still to read; a decode advances them past what it reads.
    pub const fn input(&mut self) -> &mut &'a [u8] {
        &mut self.input
    }

    /// The bytes this decode may still allocate (ADR-0002).
    pub const fn budget(&mut self) -> &mut Budget {
        self.budget
    }

    /// What the boundary offers a bound value being read.
    pub const fn offers(&mut self) -> &mut B::ReaderOffers<'a> {
        &mut self.offers
    }
}
