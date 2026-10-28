//! The file capability (ADR-0032): one root, [`Files`], that reads files for every other actor, in named namespaces,
//! by path.
//!
//! An actor asks it with a [`ReadFile`] and is answered a [`FileRead`] once
//! the read is done. The read itself is a job only this crate can make,
//! which reads the host's [`Storage`]: a directory for each namespace
//! natively, and fetches relative to the page in a browser. Each host's crate
//! implements the storage, bootstraps the root, and serves its storage into
//! the services its runners hand every job (ADR-0034).

mod files;
mod load;
mod read;
mod storage;

pub use files::{Files, FilesConfig};
pub use read::{FileRead, ReadFile};
pub use storage::{Namespace, Reading, Storage, Unread};
