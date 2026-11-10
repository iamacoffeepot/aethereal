//! The engine end's settings (ADR-0028, ADR-0038, ADR-0040).

use crate::{PORT, Token};
use aethereal_data::Plain;
use core::error::Error;
use core::fmt;

/// The engine end's section, `link`: where its hub is, the name the hub shows, the token it links with, and its limit
/// (ADR-0038, ADR-0040). How many tools an engine holds is `Tools`'s setting (ADR-0053).
///
/// The token has no default: the kind's `Default` holds no bytes, which is no token, so an engine started without
/// `--hub-token` fails as it begins, naming the setting ([`NoToken`]).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.link.settings")]
#[settings]
pub struct LinkSettings {
    /// The hub's host: a name, or an address written out.
    #[flag]
    pub hub: String,
    /// The hub's link port.
    #[flag]
    pub port: u16,
    /// What the hub shows the agent beside this engine; not an identity.
    #[flag]
    pub name: String,
    /// The token the hub minted for this engine, 16 bytes in hexadecimal: good for one link, and required.
    #[flag(long = "hub-token")]
    pub token: Vec<u8>,
    /// The most bytes of a frame, each way: past it, a set is not written and an answer is written as a refusal.
    #[flag]
    pub most_bytes: u32,
}

impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            hub: String::from("127.0.0.1"),
            port: PORT,
            name: String::from("engine"),
            token: Vec::new(),
            most_bytes: 1 << 20,
        }
    }
}

impl LinkSettings {
    /// What the root is made from, or that the settings hold no token.
    pub fn config(&self) -> Result<LinkConfig, NoToken> {
        let bytes = <[u8; 16]>::try_from(self.token.as_slice()).map_err(|_| NoToken { bytes: self.token.len() })?;

        Ok(LinkConfig {
            hub: self.hub.clone(),
            port: self.port,
            name: self.name.clone(),
            token: Token(bytes),
            most_bytes: self.most_bytes,
        })
    }
}

/// What the engine end's root is made from: its settings, the token read as one.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.link.config")]
pub struct LinkConfig {
    pub hub: String,
    pub port: u16,
    pub name: String,
    pub token: Token,
    pub most_bytes: u32,
}

/// Why `Link` did not start: its setting `token`, `--hub-token`, is not a token's 16 bytes. An engine links only with
/// a token its hub minted (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoToken {
    /// The bytes the setting held.
    pub bytes: usize,
}

impl fmt::Display for NoToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { bytes } = self;
        write!(
            f,
            "`link`'s `token` (`--hub-token`) holds {bytes} bytes, and a token is 16: start the engine with the token \
             its hub minted"
        )
    }
}

impl Error for NoToken {}
