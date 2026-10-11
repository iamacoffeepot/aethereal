//! The bytes a decode may still allocate (ADR-0002).

use crate::DecodeError;

/// The bytes a decode may still allocate (ADR-0002: a count is not trusted).
#[derive(Debug, PartialEq, Eq)]
pub struct Budget {
    remaining: usize,
}

impl Budget {
    /// A budget that may allocate up to `bytes` bytes.
    #[must_use]
    pub const fn new(bytes: usize) -> Self {
        Self { remaining: bytes }
    }

    /// The bytes the budget still holds.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    /// Takes `bytes` from the budget, refusing when it does not hold that many.
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), DecodeError> {
        let fits = bytes <= self.remaining;
        if fits {
            self.remaining -= bytes;
            Ok(())
        } else {
            Err(DecodeError::BudgetExceeded { requested: bytes, remaining: self.remaining })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_refuses_past_the_remainder() {
        // Catches a budget that subtracts past zero and keeps decoding.
        let mut budget = Budget::new(4);
        assert_eq!(budget.charge(5), Err(DecodeError::BudgetExceeded { requested: 5, remaining: 4 }));
        assert_eq!(budget.remaining(), 4);
    }
}
