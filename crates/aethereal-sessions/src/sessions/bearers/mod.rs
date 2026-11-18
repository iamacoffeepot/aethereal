//! The bearers of the sessions' route (ADR-0057): each minted for one session's container and named only to it, and
//! held by the capability, which checks each request on the container stack's port against them (ADR-0035).

#[cfg(test)]
mod tests;

use aethereal_data::Plain;
use aethereal_http::stack::Credentials;
use core::fmt;

/// A bearer of the route: 16 bytes of the host's randomness, minted for one session's container and given to it alone,
/// as its credential is (ADR-0057). It is a secret: its `Debug` writes no byte of it, and it is written out only as the
/// one value a container's environment holds, [`Bearer::hex`].
#[derive(Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.bearer")]
pub struct Bearer(pub [u8; 16]);

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(..)")
    }
}

/// The hex digits of a bearer.
const HEX_DIGITS: usize = 32;

impl Bearer {
    /// Its 32 lowercase hexadecimal digits, as a container writes it after `Bearer ` in its `Authorization` header.
    #[must_use]
    pub fn hex(&self) -> String {
        let mut hex = String::with_capacity(HEX_DIGITS);
        for byte in self.0 {
            hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
            hex.push(char::from(DIGITS[usize::from(byte & 0xf)]));
        }

        hex
    }

    /// The bearer a `Bearer` credential's `token` names: none when it is not 32 hexadecimal digits.
    #[must_use]
    pub fn of_token(token: &str) -> Option<Self> {
        let hex = token.as_bytes();
        if hex.len() != HEX_DIGITS {
            return None;
        }
        let mut bytes = [0; 16];
        for (byte, pair) in bytes.iter_mut().zip(hex.chunks_exact(2)) {
            *byte = (digit(pair[0])? << 4) | digit(pair[1])?;
        }

        Some(Self(bytes))
    }

    /// Whether it is `other`, compared in a time that does not hang on where they first differ, so a caller cannot
    /// find a bearer a digit at a time by how long each refusal takes.
    fn is(&self, other: &Self) -> bool {
        let differs = self.0.iter().zip(other.0).fold(0, |differs, (mine, theirs)| differs | (mine ^ theirs));

        differs == 0
    }
}

/// The lowercase hexadecimal digits, by their value.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// The value of one hexadecimal digit, either case: none for anything else.
const fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One bearer the capability holds, and the session it names.
#[derive(Clone, Copy, PartialEq, Eq, Plain)]
struct Minted {
    bearer: Bearer,
    session: u32,
}

/// The bearers the capability holds, each with the session it names: one each, as a session runs one container at a
/// time. A few dozen at most, one for each session that runs, looked through whole on every request. Its `Debug` writes
/// how many it holds and no bearer.
#[derive(Default, Plain)]
pub(super) struct Bearers {
    held: Vec<Minted>,
}

impl fmt::Debug for Bearers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bearers({})", self.held.len())
    }
}

impl Bearers {
    /// Holds `bearer` for `session`, in place of what it held for it: a session's requests bear only its latest
    /// container's bearer.
    pub(super) fn admit(&mut self, bearer: Bearer, session: u32) {
        self.dismiss(session);
        self.held.push(Minted { bearer, session });
    }

    /// Lets go of what it holds for `session`.
    pub(super) fn dismiss(&mut self, session: u32) {
        self.held.retain(|held| held.session != session);
    }

    /// The session a request bearing `credentials` comes from: none when they name no bearer held. A bearer is named as
    /// a `Bearer` token, or as the password of `Basic` credentials of any user, as git sends what it holds (ADR-0057).
    /// Every bearer held is compared, whichever names it.
    pub(super) fn session_of(&self, credentials: &Credentials) -> Option<u32> {
        let token = match credentials {
            Credentials::Bearer { token } | Credentials::Basic { password: token, .. } => token,
            Credentials::None | Credentials::Other { .. } => return None,
        };
        let bearer = Bearer::of_token(token)?;

        self.held.iter().fold(None, |found, held| {
            if held.bearer.is(&bearer) {
                Some(held.session)
            } else {
                found
            }
        })
    }
}
