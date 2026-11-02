//! The token table on times the tests state: a token's expiry, and the oldest dropped past the most kept.

use super::*;

/// The token of bytes all `byte`.
const fn token(byte: u8) -> Token {
    Token([byte; 16])
}

/// The time `secs` seconds in.
const fn at(secs: u64) -> Time {
    Time::from_nanos(secs * 1_000_000_000)
}

/// How long the tests' tokens last.
const WITHIN: Duration = Duration::from_secs(10);

#[test]
fn a_token_is_good_until_its_time_has_passed_and_not_at_it() {
    // Catches expiry measured from the wrong time, or compared the wrong way at its edge: a token taken at the instant
    // its time runs out would link an engine the setting says is too late.
    let mut tokens = Tokens::default();
    tokens.mint_external(token(1), at(5), WITHIN, 64);
    tokens.mint_external(token(2), at(5), WITHIN, 64);

    assert_eq!(tokens.take(token(1), Time::from_nanos(15_000_000_000 - 1)), TokenFor::External);
    assert_eq!(tokens.take(token(2), at(15)), TokenFor::Unknown);
}

#[test]
fn past_the_most_kept_the_oldest_token_is_dropped_as_another_is_minted() {
    // Catches a table that grows without bound, one that drops the newest or a token at random, and one that keeps
    // fewer than its most.
    let mut tokens = Tokens::default();
    tokens.mint_external(token(1), at(1), WITHIN, 2);
    tokens.mint_external(token(2), at(2), WITHIN, 2);
    tokens.mint_external(token(3), at(3), WITHIN, 2);

    assert_eq!(tokens.take(token(1), at(4)), TokenFor::Unknown);
    assert_eq!(tokens.take(token(2), at(4)), TokenFor::External);
    assert_eq!(tokens.take(token(3), at(4)), TokenFor::External);
}
