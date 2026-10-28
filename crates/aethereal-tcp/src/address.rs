//! A peer's address, as plain data (ADR-0033).

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
