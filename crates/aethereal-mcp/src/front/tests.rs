//! The parts of the front a request over HTTP does not reach each case of.

use super::{decoded_name, origin};

#[test]
fn a_name_in_the_base64_form_is_decoded_and_a_plain_one_is_not() {
    // Catches the sentinel taken as the name, and a plain name run through the decoder.
    assert_eq!(decoded_name("=?base64?Zm9v?="), Some(String::from("foo")));
    assert_eq!(decoded_name("Zm9v"), Some(String::from("Zm9v")));
    assert_eq!(decoded_name("=?base64?*?="), None);
}

#[test]
fn only_this_machines_origins_and_those_listed_are_allowed() {
    // Catches a name matched by prefix, as a rebinding page's `localhost.evil.test` would be, and a scheme not checked.
    let listed = [String::from("https://tool.example")];

    assert!(origin::is_allowed("http://localhost", &[]));
    assert!(origin::is_allowed("http://localhost:3000", &[]));
    assert!(origin::is_allowed("https://127.0.0.1:8890", &[]));
    assert!(origin::is_allowed("http://[::1]:8890", &[]));
    assert!(origin::is_allowed("https://tool.example", &listed));
    assert!(!origin::is_allowed("http://localhost.evil.test", &[]));
    assert!(!origin::is_allowed("http://localhost:", &[]));
    assert!(!origin::is_allowed("file://localhost", &[]));
    assert!(!origin::is_allowed("https://tool.example:1", &listed));
    assert!(!origin::is_allowed("null", &[]));
}
