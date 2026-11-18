//! What a listener does when an accept fails, read from the error alone (ADR-0033): it accepts the next connection,
//! backs off while the host is out of descriptors or memory, or unbinds, which only a broken listening socket makes it
//! do.

use std::io::{self, ErrorKind};

/// What a failed accept says, which the listener meets as named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No connection is waiting: the listener waits for its socket to be readable.
    Drained,
    /// The call was interrupted, or the one connection it would have taken failed before it was accepted and is
    /// gone: the listener accepts the next at once.
    Next,
    /// The host is out of descriptors or memory, so an accept made now fails the same way: the listener stops
    /// accepting for a back-off, and new connections wait in the kernel's backlog.
    BackOff,
    /// The listening socket itself is broken, or the error is one it does not know: no accept would succeed, and one
    /// made again at once would spin, so the listener unbinds.
    Broken,
}

impl Failure {
    /// What `error`, which an accept failed with, says: from its kind where std names one for it (`ECONNABORTED`,
    /// `ENOMEM`), and otherwise from its system error code.
    pub fn of(error: &io::Error) -> Self {
        match error.kind() {
            ErrorKind::WouldBlock => Self::Drained,
            ErrorKind::Interrupted | ErrorKind::ConnectionAborted => Self::Next,
            ErrorKind::OutOfMemory => Self::BackOff,
            _ => error.raw_os_error().map_or(Self::Broken, Self::of_code),
        }
    }

    /// What the system's error code `code` says, as accept(2) names them: those of one connection, and those of a
    /// host out of descriptors or memory, that std files under no kind of their own.
    #[cfg(unix)]
    const fn of_code(code: i32) -> Self {
        match code {
            libc::EPROTO => Self::Next,
            // Linux hands a new connection's pending network error out of accept, which accept(2) says to treat as
            // one to try again, and refuses one connection a firewall rule forbids.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            libc::ENETDOWN
            | libc::ENOPROTOOPT
            | libc::EHOSTDOWN
            | libc::ENONET
            | libc::EHOSTUNREACH
            | libc::EOPNOTSUPP
            | libc::ENETUNREACH
            | libc::EPERM => Self::Next,
            libc::EMFILE | libc::ENFILE | libc::ENOBUFS => Self::BackOff,
            _ => Self::Broken,
        }
    }

    /// A host with no system error codes names no more failures of one connection, or of a host out of descriptors or
    /// memory, than its kinds do.
    #[cfg(not(unix))]
    const fn of_code(_: i32) -> Self {
        Self::Broken
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// What an accept failed with the system's error code `code` says.
    fn failure(code: i32) -> Failure {
        Failure::of(&io::Error::from_raw_os_error(code))
    }

    #[test]
    fn only_an_error_of_the_listening_socket_itself_unbinds_it() {
        // Catches an error of one connection, or of a host out of descriptors or memory, filed as a broken socket, so
        // one failed accept closes the listener's port for good; one of exhaustion filed as one connection's, so the
        // listener spins on accepts that fail; and a broken socket filed as either, so it spins or never unbinds.
        assert_eq!(failure(libc::EAGAIN), Failure::Drained);
        for code in [libc::EINTR, libc::ECONNABORTED, libc::EPROTO] {
            assert_eq!(failure(code), Failure::Next, "{code}");
        }
        for code in [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM] {
            assert_eq!(failure(code), Failure::BackOff, "{code}");
        }
        for code in [libc::EBADF, libc::EINVAL, libc::ENOTSOCK, libc::EFAULT] {
            assert_eq!(failure(code), Failure::Broken, "{code}");
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_pending_network_error_linux_hands_out_of_accept_passes_over_one_connection() {
        // Catches one of the network errors Linux hands out of accept for a new connection filed as a broken socket,
        // so a peer whose network fails as it connects closes the listener's port for good.
        let pending = [
            libc::ENETDOWN,
            libc::ENOPROTOOPT,
            libc::EHOSTDOWN,
            libc::ENONET,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
            libc::ENETUNREACH,
            libc::EPERM,
        ];

        for code in pending {
            assert_eq!(failure(code), Failure::Next, "{code}");
        }
    }
}
