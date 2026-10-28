//! A response's status, as a closed set of named cases (ADR-0035).

use aethereal_data::Plain;

/// The status a response is written with: each a final status HTTP names (RFC 9110 §15), so a response cannot be
/// written with a number HTTP gives no meaning, or with an informational status, which the exchange writes itself.
/// A status a handler needs that is not here is a case added here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.status")]
pub enum Status {
    Ok,
    Created,
    Accepted,
    NoContent,
    MovedPermanently,
    Found,
    SeeOther,
    NotModified,
    TemporaryRedirect,
    PermanentRedirect,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    NotAcceptable,
    RequestTimeout,
    Conflict,
    Gone,
    ContentTooLarge,
    UnsupportedMediaType,
    /// The joke of RFC 2324, which RFC 9110 §15.5.19 keeps reserved so that no one gives it another meaning.
    ImATeapot,
    UnprocessableContent,
    TooManyRequests,
    RequestHeaderFieldsTooLarge,
    InternalServerError,
    NotImplemented,
    BadGateway,
    ServiceUnavailable,
    GatewayTimeout,
    HttpVersionNotSupported,
}

impl Status {
    /// Its code and reason phrase, as the status line writes them.
    #[must_use]
    pub const fn line(self) -> (u16, &'static str) {
        match self {
            Self::Ok => (200, "OK"),
            Self::Created => (201, "Created"),
            Self::Accepted => (202, "Accepted"),
            Self::NoContent => (204, "No Content"),
            Self::MovedPermanently => (301, "Moved Permanently"),
            Self::Found => (302, "Found"),
            Self::SeeOther => (303, "See Other"),
            Self::NotModified => (304, "Not Modified"),
            Self::TemporaryRedirect => (307, "Temporary Redirect"),
            Self::PermanentRedirect => (308, "Permanent Redirect"),
            Self::BadRequest => (400, "Bad Request"),
            Self::Unauthorized => (401, "Unauthorized"),
            Self::Forbidden => (403, "Forbidden"),
            Self::NotFound => (404, "Not Found"),
            Self::MethodNotAllowed => (405, "Method Not Allowed"),
            Self::NotAcceptable => (406, "Not Acceptable"),
            Self::RequestTimeout => (408, "Request Timeout"),
            Self::Conflict => (409, "Conflict"),
            Self::Gone => (410, "Gone"),
            Self::ContentTooLarge => (413, "Content Too Large"),
            Self::UnsupportedMediaType => (415, "Unsupported Media Type"),
            Self::ImATeapot => (418, "I'm a teapot"),
            Self::UnprocessableContent => (422, "Unprocessable Content"),
            Self::TooManyRequests => (429, "Too Many Requests"),
            Self::RequestHeaderFieldsTooLarge => (431, "Request Header Fields Too Large"),
            Self::InternalServerError => (500, "Internal Server Error"),
            Self::NotImplemented => (501, "Not Implemented"),
            Self::BadGateway => (502, "Bad Gateway"),
            Self::ServiceUnavailable => (503, "Service Unavailable"),
            Self::GatewayTimeout => (504, "Gateway Timeout"),
            Self::HttpVersionNotSupported => (505, "HTTP Version Not Supported"),
        }
    }

    /// Whether a response of it carries a body, and so a `Content-Length`: every one but `204` and `304` does
    /// (RFC 9110 §8.6).
    #[must_use]
    pub const fn has_body(self) -> bool {
        !matches!(self, Self::NoContent | Self::NotModified)
    }
}
