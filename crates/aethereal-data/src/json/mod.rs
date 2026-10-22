//! Plain data as JSON text, from a schema and bytes (ADR-0007).
//!
//! The text is a view of the bytes of ADR-0002 for people and tools;
//! mail between actors stays bytes. Each shape has one JSON form,
//! 64-bit integers are strings, and both walks follow the schema.

mod error;
pub mod escape;
mod float;
mod from_json;
mod hex;
mod integer;
mod key;
pub mod layout;
pub mod reader;
#[cfg(test)]
mod rows;
mod to_json;
pub mod tree;

pub use error::{JsonError, JsonFault, JsonForm, JsonPath, JsonStep, TextFault};
pub use from_json::from_json;
pub use to_json::to_json;
