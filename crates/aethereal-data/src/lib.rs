//! Plain data: values that are only their bytes.
//!
//! The byte layout is ADR-0002, the schema vocabulary ADR-0003, and the
//! `Plain` trait with the Rust types that implement it ADR-0004.

#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;
