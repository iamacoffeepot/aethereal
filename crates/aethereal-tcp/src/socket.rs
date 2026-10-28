//! The sockets the capability's actors own (ADR-0010, ADR-0033).

use aethereal_actor::Resource;
use std::net::{TcpListener, TcpStream};

/// A connection's socket, set not to block: owned by the connection actor that reads and writes it.
pub struct Socket(pub TcpStream);

impl Resource for Socket {}

/// A listener's socket, bound and set not to block: owned by the listener that accepts on it.
pub struct BoundSocket(pub TcpListener);

impl Resource for BoundSocket {}
