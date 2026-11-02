//! The token table on times the tests state: a token's expiry, and the oldest dropped past the most kept.

use super::*;
use aethereal_actor::{SeatNumber, Seated};
use aethereal_data::ActorId;
use aethereal_store::StoreName;

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

/// The hash of the build the tests' spawns run.
const HASH: StoreHash = StoreHash([9; 32]);

/// A reference standing for the spawn numbered `number`: the table only keeps it, so no actor need be there.
const fn spawned(number: u64) -> ActorRef<Spawned> {
    ActorRef::found(Seated { id: ActorId::new(number), seat: SeatNumber::new(0) })
}

/// The binary the tests' spawns were asked for.
fn binary() -> Binary {
    let Ok(name) = StoreName::parse("engine") else {
        panic!("the tests' name conforms");
    };

    Binary::Named(name)
}

/// What taking `token` at `now` finds it was for.
fn taken(tokens: &mut Tokens, token: Token, now: Time) -> TokenFor {
    match tokens.take(token, now) {
        Some(Purpose::External { .. }) => TokenFor::External,
        Some(Purpose::Spawn { .. }) => TokenFor::Spawned,
        None => TokenFor::Unknown,
    }
}

#[test]
fn a_token_is_good_until_its_time_has_passed_and_not_at_it() {
    // Catches expiry measured from the wrong time, or compared the wrong way at its edge: a token taken at the instant
    // its time runs out would link an engine the setting says is too late.
    let mut tokens = Tokens::default();
    tokens.mint_external(token(1), at(5), WITHIN, 64);
    tokens.mint_external(token(2), at(5), WITHIN, 64);

    assert_eq!(taken(&mut tokens, token(1), Time::from_nanos(15_000_000_000 - 1)), TokenFor::External);
    assert_eq!(taken(&mut tokens, token(2), at(15)), TokenFor::Unknown);
}

#[test]
fn past_the_most_kept_the_oldest_token_is_dropped_as_another_is_minted() {
    // Catches a table that grows without bound, one that drops the newest or a token at random, and one that keeps
    // fewer than its most.
    let mut tokens = Tokens::default();
    tokens.mint_external(token(1), at(1), WITHIN, 2);
    tokens.mint_external(token(2), at(2), WITHIN, 2);
    tokens.mint_external(token(3), at(3), WITHIN, 2);

    assert_eq!(taken(&mut tokens, token(1), at(4)), TokenFor::Unknown);
    assert_eq!(taken(&mut tokens, token(2), at(4)), TokenFor::External);
    assert_eq!(taken(&mut tokens, token(3), at(4)), TokenFor::External);
}

#[test]
fn a_spawns_token_lasts_until_its_spawn_forgets_it_and_is_never_dropped_for_room() {
    // Catches a spawn's token that expires as one for an engine started by hand does, so a slow engine could never
    // link, one dropped to make room for those, or counted against their most, and one still good once its spawn has
    // ended.
    let mut tokens = Tokens::default();
    tokens.mint_spawn(token(1), spawned(0), binary(), HASH);
    tokens.mint_spawn(token(2), spawned(1), binary(), HASH);
    tokens.mint_external(token(3), at(100), WITHIN, 1);
    tokens.mint_external(token(4), at(100), WITHIN, 1);
    tokens.forget(token(2));

    let first = Purpose::Spawn { spawned: spawned(0), binary: binary(), hash: HASH };
    assert_eq!(tokens.take(token(1), at(1_000)), Some(first));
    assert_eq!(taken(&mut tokens, token(2), at(100)), TokenFor::Unknown);
    assert_eq!(taken(&mut tokens, token(3), at(100)), TokenFor::Unknown);
    assert_eq!(taken(&mut tokens, token(4), at(100)), TokenFor::External);
}
