//! A request's head as a stack's layers are handed it (ADR-0035), and the credentials read from its one
//! `Authorization` header.

use crate::base64;
use crate::head::{Malformed, is_token};
use crate::{Header, Method};
use aethereal_data::Plain;
use aethereal_tcp::SocketAddress;
use core::fmt;

/// A request's head, as the stack parsed it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Head {
    pub method: Method,
    /// The target's path, as sent.
    pub path: String,
    /// What follows the target's `?`, empty when there is none.
    pub query: String,
    /// In the order sent, duplicates kept, each name lowercased, but `Authorization`, which is read into
    /// `credentials`.
    pub headers: Vec<Header>,
    /// Read from the one `Authorization` header.
    pub credentials: Credentials,
    /// Where the request came from.
    pub peer: SocketAddress,
}

/// What a request's one `Authorization` header says of who sent it (RFC 7235), its scheme named in any case. A
/// credential is a secret: its `Debug` names its scheme and no more.
#[derive(Clone, PartialEq, Eq, Plain)]
pub enum Credentials {
    /// The request has no `Authorization` header.
    None,
    /// RFC 6750.
    Bearer { token: String },
    /// RFC 7617: decoded from Base64 and split at the first `:`.
    Basic { user: String, password: String },
    /// Any other scheme, lowercased, and the rest of the header as sent.
    Other { scheme: String, parameters: String },
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Bearer { .. } => f.write_str("Bearer"),
            Self::Basic { .. } => f.write_str("Basic"),
            Self::Other { scheme, .. } => write!(f, "Other({scheme})"),
        }
    }
}

impl Credentials {
    /// The credentials of the one `Authorization` header among `headers`, which it is taken out of: `None` when there
    /// is none.
    ///
    /// # Errors
    ///
    /// [`Malformed`] when `Authorization` is sent more than once, its scheme is not a token, or `Basic` credentials do
    /// not decode.
    pub(super) fn taken(headers: &mut Vec<Header>) -> Result<Self, Malformed> {
        let mut named = headers.iter().enumerate().filter(|(_, header)| header.name == "authorization");
        let at = match (named.next(), named.next()) {
            (None, _) => return Ok(Self::None),
            (Some((at, _)), None) => at,
            (Some(_), Some(_)) => return Err(Malformed),
        };

        Self::read(&headers.remove(at).value)
    }

    /// The credentials an `Authorization` header's value says: its scheme, then, past one or more spaces, the rest.
    fn read(value: &str) -> Result<Self, Malformed> {
        let (scheme, rest) = value.split_once(' ').unwrap_or((value, ""));
        let rest = rest.trim_start_matches(' ');
        let is_scheme = !scheme.is_empty() && is_token(scheme.as_bytes());
        if !is_scheme {
            return Err(Malformed);
        }

        if scheme.eq_ignore_ascii_case("bearer") {
            return Ok(Self::Bearer { token: String::from(rest) });
        }
        if scheme.eq_ignore_ascii_case("basic") {
            return basic(rest);
        }

        Ok(Self::Other { scheme: scheme.to_ascii_lowercase(), parameters: String::from(rest) })
    }
}

/// The `Basic` credentials `encoded` writes in Base64: a user and a password, split at the first `:`.
fn basic(encoded: &str) -> Result<Credentials, Malformed> {
    let decoded = base64::decode(encoded).ok_or(Malformed)?;
    let pair = String::from_utf8(decoded).map_err(|_| Malformed)?;
    let (user, password) = pair.split_once(':').ok_or(Malformed)?;

    Ok(Credentials::Basic { user: String::from(user), password: String::from(password) })
}

#[cfg(test)]
mod tests {
    use super::Credentials;
    use crate::Header;
    use crate::head::Malformed;

    /// What `values`, each sent as an `Authorization` header beside a `Host`, read as, and the headers left.
    fn taken(values: &[&str]) -> (Result<Credentials, Malformed>, Vec<Header>) {
        let mut headers = vec![Header::new("host", "here")];
        headers.extend(values.iter().map(|value| Header::new("authorization", value)));
        let credentials = Credentials::taken(&mut headers);

        (credentials, headers)
    }

    #[test]
    fn each_scheme_is_read_in_any_case_and_the_header_is_taken_out() {
        // Catches a scheme matched only as `Bearer` is written, so a client that writes `bearer` is read as another
        // scheme; a `Basic` password cut at a second `:`; another scheme left in the case it came, so a layer's match
        // misses it; and the header left among the ones a handler is handed, with its secret.
        let (bearer, left) = taken(&["bearer  abc.def"]);
        assert_eq!(bearer, Ok(Credentials::Bearer { token: String::from("abc.def") }));
        assert_eq!(left, [Header::new("host", "here")]);

        // `YWxpY2U6czNjcjp0` is `alice:s3cr:t`.
        let (basic, _) = taken(&["BASIC YWxpY2U6czNjcjp0"]);
        let alice = Credentials::Basic { user: String::from("alice"), password: String::from("s3cr:t") };
        assert_eq!(basic, Ok(alice));

        let (other, _) = taken(&["Digest username=\"a\", realm=\"b\""]);
        let digest = Credentials::Other {
            scheme: String::from("digest"),
            parameters: String::from("username=\"a\", realm=\"b\""),
        };
        assert_eq!(other, Ok(digest));

        assert_eq!(taken(&[]).0, Ok(Credentials::None));
    }

    #[test]
    fn credentials_sent_twice_or_that_do_not_decode_are_malformed() {
        // Catches two `Authorization` headers settled in favour of one, which two readers could settle differently,
        // and `Basic` credentials that are not Base64, or hold no `:`, read as a user with no password.
        assert_eq!(taken(&["Bearer a", "Bearer b"]).0, Err(Malformed));
        assert_eq!(taken(&["Basic not*base64"]).0, Err(Malformed));
        // `YWxpY2U=` is `alice`.
        assert_eq!(taken(&["Basic YWxpY2U="]).0, Err(Malformed));
        assert_eq!(taken(&["(bad) token"]).0, Err(Malformed));
    }

    #[test]
    fn a_credential_is_never_written_by_its_debug() {
        // Catches a derived `Debug` that writes a bearer token or a password into a log.
        let basic = Credentials::Basic { user: String::from("alice"), password: String::from("s3cret") };
        let bearer = Credentials::Bearer { token: String::from("t0ken") };

        assert_eq!(format!("{basic:?} {bearer:?}"), "Basic Bearer");
    }
}
