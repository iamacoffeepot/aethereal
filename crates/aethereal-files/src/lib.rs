//! The file capability (ADR-0032): one root, [`Files`], that reads files for every other actor, in named namespaces,
//! by path.
//!
//! An actor asks it with a [`ReadFile`] and is answered a [`FileRead`] once
//! the read is done. The read itself is a job only this crate can make,
//! which reads the host's storage (`aethereal_actor::Storage`): a directory
//! for each namespace natively, and fetches relative to the page in a
//! browser. Each host's crate bootstraps the root, and serves its host a
//! [`FileHosting`]: its storage, and the job the root runs.

mod files;
mod host;
mod load;
mod read;

pub use files::{Files, FilesConfig};
pub use host::FileHosting;
pub use read::{FileRead, ReadFile};
