//! The Docker client's settings (ADR-0028, ADR-0057).

use aethereal_data::Plain;
use aethereal_tcp::{SocketPath, Unaddressable};
use core::error::Error;
use core::fmt;

/// The Docker client's section, `docker`: where Docker's socket is, and how far ahead of its asker it reads a stream.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.docker.settings")]
#[settings]
pub struct DockerSettings {
    /// The path of Docker's Unix socket.
    #[flag]
    pub socket: String,
    /// The parts of a response the client reads ahead of what it has passed on: as many of Docker's reads are held at
    /// once for each stream whose asker is behind.
    #[flag]
    pub window: u32,
}

impl Default for DockerSettings {
    fn default() -> Self {
        Self { socket: String::from("/var/run/docker.sock"), window: 4 }
    }
}

impl DockerSettings {
    /// What the root is made from.
    ///
    /// # Errors
    ///
    /// Why the socket's path can name no socket.
    pub fn config(&self) -> Result<DockerConfig, SocketUnaddressable> {
        let socket = SocketPath::new(self.socket.clone())
            .map_err(|why| SocketUnaddressable { socket: self.socket.clone(), why })?;

        Ok(DockerConfig { socket, window: self.window })
    }
}

/// What the Docker client's root is made from: Docker's socket, and how far ahead it reads a stream.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.docker.config")]
pub struct DockerConfig {
    pub socket: SocketPath,
    pub window: u32,
}

/// Why `Docker` did not start: its socket's path can name no socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketUnaddressable {
    /// The path the settings name.
    pub socket: String,
    pub why: Unaddressable,
}

impl fmt::Display for SocketUnaddressable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { socket, why } = self;
        write!(f, "`docker.socket` is `{socket}`, which names no socket: {why:?}")
    }
}

impl Error for SocketUnaddressable {}
