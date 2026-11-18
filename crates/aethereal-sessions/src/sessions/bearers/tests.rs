use super::{Bearer, Bearers};
use aethereal_http::stack::Credentials;

/// A bearer of the bytes `0..16`.
const BEARER: Bearer = Bearer([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

/// The credentials of a request bearing `bearer`.
fn bearing(bearer: Bearer) -> Credentials {
    Credentials::Bearer { token: bearer.hex() }
}

#[test]
fn a_bearer_reads_back_from_its_token_and_nothing_else_does() {
    // Catches the digits written in an order the token is not read in, a token of the wrong length read as some
    // bearer, and a digit outside hexadecimal read as a value.
    let written = BEARER.hex();

    assert_eq!(Bearer::of_token(&written), Some(BEARER));
    assert_eq!(Bearer::of_token(&written.to_uppercase()), Some(BEARER));
    assert_eq!(Bearer::of_token(&format!("{written}0")), None);
    assert_eq!(Bearer::of_token(&written[..written.len() - 1]), None);
    assert_eq!(Bearer::of_token(&written.replace('f', "g")), None);
}

#[test]
fn a_session_admitted_again_is_named_only_by_its_latest_bearer() {
    // Catches a session's bearer of a container it no longer runs still let in, a dismissal that lets go of another
    // session's bearer, a bearer sent as `Basic`'s password, as git sends it, refused, and a `Basic` user read as one.
    let older = Bearer([7; 16]);
    let other = Bearer([9; 16]);
    let mut bearers = Bearers::default();
    bearers.admit(older, 3);
    bearers.admit(BEARER, 3);
    bearers.admit(other, 4);

    assert_eq!(bearers.session_of(&bearing(older)), None);
    assert_eq!(bearers.session_of(&bearing(BEARER)), Some(3));
    let as_password = Credentials::Basic { user: String::from("git"), password: BEARER.hex() };
    assert_eq!(bearers.session_of(&as_password), Some(3));
    let as_user = Credentials::Basic { user: BEARER.hex(), password: String::new() };
    assert_eq!(bearers.session_of(&as_user), None);

    bearers.dismiss(3);

    assert_eq!(bearers.session_of(&bearing(BEARER)), None);
    assert_eq!(bearers.session_of(&bearing(other)), Some(4));
}
