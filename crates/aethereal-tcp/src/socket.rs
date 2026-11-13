//! The sockets the capability's actors own (ADR-0010, ADR-0033).

use aethereal_actor::Resource;
use std::io::{self, IoSlice, Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// A connection's socket, set not to block: owned by the connection actor that reads and writes it. A connection reads
/// and writes either kind the same way, so the actor that holds it never asks which it is.
pub enum Socket {
    /// A connection over TCP.
    Tcp(TcpStream),
    /// A connection to a Unix socket on this machine.
    #[cfg(unix)]
    Unix(UnixStream),
}

impl Resource for Socket {}

/// A listener's socket, bound and set not to block: owned by the listener that accepts on it.
pub struct BoundSocket(pub TcpListener);

impl Resource for BoundSocket {}

impl Socket {
    /// Its descriptor, as a [`Ready`](crate::ready::Ready) names it.
    pub(crate) fn descriptor(&self) -> u64 {
        match self {
            Self::Tcp(stream) => descriptor(stream),
            #[cfg(unix)]
            Self::Unix(stream) => descriptor(stream),
        }
    }
}

impl BoundSocket {
    /// Its descriptor, as a [`Ready`](crate::ready::Ready) names it.
    pub(crate) fn descriptor(&self) -> u64 {
        descriptor(&self.0)
    }
}

/// A socket is read through `&`, as each of its kinds is: the kernel reads a socket through a shared reference.
impl Read for &Socket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        // Each kind reads through `&`, and its methods take `&mut self`: the reference, bound `mut`, is the `self`.
        match self {
            Socket::Tcp(stream) => {
                let mut stream: &TcpStream = stream;
                stream.read(bytes)
            }
            #[cfg(unix)]
            Socket::Unix(stream) => {
                let mut stream: &UnixStream = stream;
                stream.read(bytes)
            }
        }
    }
}

/// A socket is written through `&`, as each of its kinds is. A gathered write is passed on whole, so it stays one
/// system call.
impl Write for &Socket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Socket::Tcp(stream) => {
                let mut stream: &TcpStream = stream;
                stream.write(bytes)
            }
            #[cfg(unix)]
            Socket::Unix(stream) => {
                let mut stream: &UnixStream = stream;
                stream.write(bytes)
            }
        }
    }

    fn write_vectored(&mut self, slices: &[IoSlice<'_>]) -> io::Result<usize> {
        match self {
            Socket::Tcp(stream) => {
                let mut stream: &TcpStream = stream;
                stream.write_vectored(slices)
            }
            #[cfg(unix)]
            Socket::Unix(stream) => {
                let mut stream: &UnixStream = stream;
                stream.write_vectored(slices)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Socket::Tcp(stream) => {
                let mut stream: &TcpStream = stream;
                stream.flush()
            }
            #[cfg(unix)]
            Socket::Unix(stream) => {
                let mut stream: &UnixStream = stream;
                stream.flush()
            }
        }
    }
}

/// The descriptor of `socket`: on a host whose sockets are file descriptors, its own.
#[cfg(unix)]
fn descriptor(socket: &impl AsRawFd) -> u64 {
    // A descriptor is never negative; one that were would name no socket, and be waited on as none.
    u64::try_from(socket.as_raw_fd()).unwrap_or(u64::MAX)
}

/// The descriptor of `socket`, on a host whose sockets are not file descriptors: none a poller knows, so a wait for it
/// is ready at once, and the host serves no poller to wait in anyway.
#[cfg(not(unix))]
const fn descriptor<T>(_: &T) -> u64 {
    u64::MAX
}
