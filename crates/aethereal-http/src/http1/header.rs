//! A header of a request or a response (RFC 9110 §5).

use aethereal_data::Plain;

/// One header: its name, lowercased in a request, and its value.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.header")]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    /// The header `name: value`.
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        Self { name: String::from(name), value: String::from(value) }
    }
}
