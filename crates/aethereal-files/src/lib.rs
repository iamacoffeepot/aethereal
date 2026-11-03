//! The file capability (ADR-0032, ADR-0039): one root, [`Files`], that reads and writes files for every other actor, in
//! named namespaces, by path.
//!
//! An actor asks it with a [`ReadFile`] and is answered a [`FileRead`] once
//! the read is done, or with a [`WriteFile`] and is answered a
//! [`FileWritten`]. Each read and write is a job only this crate can make,
//! done through the host's [`Storage`]: a directory for each namespace
//! natively, and fetches relative to the page and the origin private file
//! system in a browser. Each host's crate
//! implements the storage, bootstraps the root, and serves its storage into
//! the services its runners hand every job (ADR-0034).

mod files;
mod load;
mod read;
mod save;
mod storage;
#[cfg(feature = "tools")]
mod tool;
mod write;

pub use files::{Files, FilesConfig};
pub use read::{FileRead, ReadFile};
pub use storage::{Namespace, Read, Reading, Storage, Unread, Unwritten, Writing};
#[cfg(feature = "tools")]
pub use tool::{Calling, FileContents, ReadTool, ReadToolConfig, TextFile, WriteTool, WriteToolConfig};
pub use write::{FileWritten, WriteFile};
