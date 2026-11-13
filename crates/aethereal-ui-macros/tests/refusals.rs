//! What the macros refuse themselves (ADR-0056: Expanded by a macro): malformed markup, a reference outside the subset,
//! and, within a `widget!`, a name the composite does not declare, each a `compile_error!` at the offending token with
//! a plain message, compared with the message each case under `refusals/` is written to get.

#[test]
fn the_macros_refuse_what_they_check_at_the_offending_token() {
    trybuild::TestCases::new().compile_fail("tests/refusals/*.rs");
}
