//! Where a connection reaches, as plain data: a peer's address, and a Unix socket's path (ADR-0033).

use aethereal_data::Plain;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// An IPv4 or IPv6 address and a port: the other end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.socket_address")]
pub enum SocketAddress {
    /// An IPv4 address.
    V4 { octets: [u8; 4], port: u16 },
    /// An IPv6 address.
    V6 { octets: [u8; 16], port: u16 },
}

impl From<SocketAddr> for SocketAddress {
    fn from(address: SocketAddr) -> Self {
        match address.ip() {
            IpAddr::V4(ip) => Self::V4 { octets: ip.octets(), port: address.port() },
            IpAddr::V6(ip) => Self::V6 { octets: ip.octets(), port: address.port() },
        }
    }
}

impl From<SocketAddress> for SocketAddr {
    fn from(address: SocketAddress) -> Self {
        match address {
            SocketAddress::V4 { octets, port } => Self::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
            SocketAddress::V6 { octets, port } => Self::new(IpAddr::V6(Ipv6Addr::from(octets)), port),
        }
    }
}

/// The most bytes a Unix socket's address holds of its path, `sun_path`: 108 on Linux, and 104 on macOS and the BSDs,
/// counting the zero that ends the path.
#[cfg(any(target_os = "linux", target_os = "android"))]
const SUN_PATH_BYTES: usize = 108;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const SUN_PATH_BYTES: usize = 104;

/// Where a Unix socket is on this machine: an absolute path that fits a socket's address (ADR-0033).
///
/// A path made here is checked; one that came in mail was decoded, not made here, so the root checks it again before
/// it dials.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.socket_path")]
pub struct SocketPath {
    path: String,
}

/// Why no Unix socket can be reached at a path. A path is refused whole, never cut to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.unaddressable")]
pub enum Unaddressable {
    /// The path does not start at the root: where it leads would hang on where the engine runs.
    Relative,
    /// The path is `bytes` long, past the `most_bytes` a socket's address holds.
    TooLong { bytes: u32, most_bytes: u32 },
    /// The path holds a zero byte, which would end it early.
    Zero,
}

impl SocketPath {
    /// The most bytes of a path: what a socket's address holds, less the zero that ends it.
    pub const MOST_BYTES: usize = SUN_PATH_BYTES - 1;

    /// The socket at `path`, or why none can be reached there.
    ///
    /// # Errors
    ///
    /// Why `path` cannot name a socket: it is relative, longer than [`MOST_BYTES`](Self::MOST_BYTES), or holds a zero
    /// byte.
    pub fn new(path: String) -> Result<Self, Unaddressable> {
        check(&path)?;

        Ok(Self { path })
    }

    /// The path, checked again: it may have come in mail.
    pub(crate) fn checked(&self) -> Result<&str, Unaddressable> {
        check(&self.path)?;

        Ok(&self.path)
    }
}

/// Whether `path` can name a Unix socket, and if not, why not.
fn check(path: &str) -> Result<(), Unaddressable> {
    let is_absolute = path.starts_with('/');
    if !is_absolute {
        return Err(Unaddressable::Relative);
    }
    let bytes = path.len();
    if bytes > SocketPath::MOST_BYTES {
        let bytes = u32::try_from(bytes).unwrap_or(u32::MAX);
        let most_bytes = u32::try_from(SocketPath::MOST_BYTES).unwrap_or(u32::MAX);
        return Err(Unaddressable::TooLong { bytes, most_bytes });
    }
    if path.contains('\0') {
        return Err(Unaddressable::Zero);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path of `bytes` bytes: the root, then `s` to its end.
    fn path_of(bytes: usize) -> String {
        format!("/{}", "s".repeat(bytes - 1))
    }

    #[test]
    fn a_path_is_refused_whole_when_it_is_relative_too_long_or_holds_a_zero() {
        // Catches a relative path dialed from wherever the engine runs, a limit that forgets the zero ending the path
        // (so the longest path passes here and fails at the dial), a long path cut to fit and dialed somewhere else,
        // and a zero that ends the path early.
        let longest = SocketPath::MOST_BYTES;

        assert_eq!(SocketPath::new(String::from("docker.sock")), Err(Unaddressable::Relative));
        assert_eq!(SocketPath::new(String::from("./docker.sock")), Err(Unaddressable::Relative));
        assert!(SocketPath::new(path_of(longest)).is_ok(), "the longest path fits");
        let too_long = u32::try_from(longest + 1).unwrap_or(u32::MAX);
        let most_bytes = u32::try_from(longest).unwrap_or(u32::MAX);
        let refused = Unaddressable::TooLong { bytes: too_long, most_bytes };
        assert_eq!(SocketPath::new(path_of(longest + 1)), Err(refused));
        assert_eq!(SocketPath::new(String::from("/run/a\0b.sock")), Err(Unaddressable::Zero));
    }

    #[test]
    fn a_path_that_came_in_mail_is_checked_again() {
        // Catches a root that trusts a path because its type says it was checked: decoded mail never passed `new`.
        let decoded = SocketPath { path: String::from("docker.sock") };

        assert_eq!(decoded.checked(), Err(Unaddressable::Relative));
    }

    #[cfg(unix)]
    #[test]
    fn the_longest_path_allowed_is_the_longest_the_host_binds() {
        // Catches a limit set for another system: a socket's address holds 108 bytes on Linux and 104 on macOS, so a
        // limit too high lets through a path the dial then fails on, and one too low refuses a socket that is there.
        use std::os::unix::net::UnixListener;
        use std::{env, fs, process};

        let dir = env::temp_dir().join(format!("aethereal-sun-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");
        let prefix = format!("{}/", dir.display());
        let Some(name) = SocketPath::MOST_BYTES.checked_sub(prefix.len()) else {
            panic!("the temporary directory leaves room for a socket's name");
        };
        let longest = format!("{prefix}{}", "s".repeat(name));
        let past = format!("{longest}s");

        let bound = UnixListener::bind(&longest).map(drop);
        let past_bound = UnixListener::bind(&past).map(drop);
        let _ = fs::remove_dir_all(&dir);

        assert!(SocketPath::new(longest).is_ok(), "the longest path is allowed");
        assert!(bound.is_ok(), "the host binds the longest path allowed");
        assert!(past_bound.is_err(), "the host binds no path longer");
    }
}
