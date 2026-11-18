//! A request's method (RFC 9110 §9).

use aethereal_data::Plain;

/// The methods a stack takes, and a client writes. A request of any other is answered `501` before it reaches a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.method")]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

impl Method {
    /// The method `token` names: none when it is not one a stack takes.
    #[must_use]
    pub fn named(token: &[u8]) -> Option<Self> {
        let method = match token {
            b"GET" => Self::Get,
            b"HEAD" => Self::Head,
            b"POST" => Self::Post,
            b"PUT" => Self::Put,
            b"DELETE" => Self::Delete,
            b"PATCH" => Self::Patch,
            b"OPTIONS" => Self::Options,
            _ => return None,
        };

        Some(method)
    }

    /// The token a request line writes it as.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Options => "OPTIONS",
        }
    }
}
