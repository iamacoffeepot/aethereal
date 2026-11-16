use super::{Admitted, Bearer};
use aethereal_tools::Caller;

/// A bearer of the bytes `0..16`.
const BEARER: Bearer = Bearer([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

#[test]
fn a_bearer_reads_back_from_its_header_and_nothing_else_does() {
    // Catches the digits written in an order the header is not read in, a header of the wrong scheme or length read
    // as some bearer, and a digit outside hexadecimal read as a value.
    let written = format!("Bearer {}", BEARER.hex());

    assert_eq!(Bearer::of_header(&written), Some(BEARER));
    assert_eq!(Bearer::of_header(&written.to_uppercase().replace("BEARER", "Bearer")), Some(BEARER));
    assert_eq!(Bearer::of_header(&BEARER.hex()), None);
    assert_eq!(Bearer::of_header(&format!("Basic {}", BEARER.hex())), None);
    assert_eq!(Bearer::of_header(&format!("{written}0")), None);
    assert_eq!(Bearer::of_header(&written[..written.len() - 1]), None);
    assert_eq!(Bearer::of_header(&written.replace('f', "g")), None);
}

#[test]
fn a_caller_admitted_again_is_named_only_by_its_latest_bearer() {
    // Catches a session's bearer of a container it no longer runs still let in, and a dismissal that lets go of
    // another caller's bearer.
    let older = Bearer([7; 16]);
    let other = Bearer([9; 16]);
    let session = Caller::Session { session: 3 };
    let mut admitted = Admitted::default();
    admitted.admit(older, session);
    admitted.admit(BEARER, session);
    admitted.admit(other, Caller::Session { session: 4 });

    assert_eq!(admitted.caller(&format!("Bearer {}", older.hex())), None);
    assert_eq!(admitted.caller(&format!("Bearer {}", BEARER.hex())), Some(session));

    admitted.dismiss(session);

    assert_eq!(admitted.caller(&format!("Bearer {}", BEARER.hex())), None);
    assert_eq!(admitted.caller(&format!("Bearer {}", other.hex())), Some(Caller::Session { session: 4 }));
}
