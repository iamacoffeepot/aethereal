//! The parts of the front a request over HTTP does not reach each case of.

use super::{base64, decoded_name, origin};

#[test]
fn base64_reads_every_padding_and_refuses_what_is_not_it() {
    // Catches a group's bits shifted short, padding counted as data, and a stray digit or length taken.
    assert_eq!(base64::decode(""), Some(Vec::new()));
    assert_eq!(base64::decode("Zg=="), Some(b"f".to_vec()));
    assert_eq!(base64::decode("Zm8="), Some(b"fo".to_vec()));
    assert_eq!(base64::decode("Zm9v"), Some(b"foo".to_vec()));
    assert_eq!(base64::decode("Zm9vYg=="), Some(b"foob".to_vec()));
    assert_eq!(base64::decode("Zm9"), None);
    assert_eq!(base64::decode("Zm9*"), None);
    assert_eq!(base64::decode("Z==="), None);
    assert_eq!(base64::decode("Zh=="), None);
}

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
