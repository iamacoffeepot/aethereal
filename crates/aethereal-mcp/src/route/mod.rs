//! The sessions' route (ADR-0057): a second port, bound where containers reach it, on which every call bears a bearer
//! the hub minted for one caller and named only to it. The root keeps each bearer it was told of, and a call that bears
//! none of them is refused before it is read.

#[cfg(test)]
mod tests;

use aethereal_data::Plain;
use aethereal_tools::Caller;
use core::fmt;

/// A bearer of the route: 16 bytes of the host's randomness, minted for one caller and given to it alone, as a
/// session's container is given its credential (ADR-0057). It is a secret: its `Debug` writes no byte of it, and it is
/// written out only as the one value a container's environment holds, [`Bearer::hex`].
#[derive(Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.bearer")]
pub struct Bearer(pub [u8; 16]);

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(..)")
    }
}

/// The hex digits of a bearer.
const HEX_DIGITS: usize = 32;

/// The scheme an `Authorization` header names a bearer by.
const SCHEME: &str = "Bearer ";

impl Bearer {
    /// Its 32 lowercase hexadecimal digits, as a caller writes it after `Bearer ` in its `Authorization` header.
    #[must_use]
    pub fn hex(&self) -> String {
        let mut hex = String::with_capacity(HEX_DIGITS);
        for byte in self.0 {
            hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
            hex.push(char::from(DIGITS[usize::from(byte & 0xf)]));
        }

        hex
    }

    /// The bearer an `Authorization` header's `value` names: none when it is not `Bearer ` and 32 hexadecimal digits.
    #[must_use]
    pub fn of_header(value: &str) -> Option<Self> {
        let hex = value.strip_prefix(SCHEME)?.as_bytes();
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

/// Tells the root that a call on its route bearing `bearer` comes from `caller`: any bearer the root held for `caller`
/// before is let go of, so a session's calls bear only the bearer of its latest container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.admit")]
pub struct Admit {
    pub bearer: Bearer,
    pub caller: Caller,
}

/// Tells the root to let go of the bearer it holds for `caller`: a call bearing it is refused from then on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.dismiss")]
pub struct Dismiss {
    pub caller: Caller,
}

/// The bearers the root was told of, each with the caller it names: one each, as a caller holds one at a time. A few
/// dozen at most, one for each session, looked through whole on every call.
#[derive(Debug, Default, Plain)]
pub struct Admitted {
    held: Vec<Admit>,
}

impl Admitted {
    /// Holds `bearer` for `caller`, in place of what it held for it.
    pub fn admit(&mut self, bearer: Bearer, caller: Caller) {
        self.dismiss(caller);
        self.held.push(Admit { bearer, caller });
    }

    /// Lets go of what it holds for `caller`.
    pub fn dismiss(&mut self, caller: Caller) {
        self.held.retain(|held| held.caller != caller);
    }

    /// The caller a call whose `Authorization` header is `header` comes from: none when it names no bearer held.
    /// Every bearer held is compared, whichever names it.
    #[must_use]
    pub fn caller(&self, header: &str) -> Option<Caller> {
        let bearer = Bearer::of_header(header)?;

        self.held.iter().fold(None, |found, held| {
            if held.bearer.is(&bearer) {
                Some(held.caller)
            } else {
                found
            }
        })
    }
}
