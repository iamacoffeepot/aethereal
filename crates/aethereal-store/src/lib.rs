//! The store (ADR-0039): one root, [`Store`], that keeps binaries checked in from a path on the host's machine, each
//! named by the BLAKE3 hash of its bytes, under names that move to the latest build, pruned to a byte budget.
//!
//! An actor checks a file in with a [`CheckIn`], lists the entries with
//! [`ListStored`], keeps one from pruning with a [`Pin`] or, for as long as
//! it lives, a [`Hold`], and asks for a copy of one to run with a [`Launch`]
//! (ADR-0040), which holds it too. No ask gives an entry's bytes back. Each
//! copy, write, and removal is a job only this crate makes, done through the
//! host's [`Stockroom`]: a directory natively. A native host's crate
//! implements it, bootstraps the root, and serves its stockroom into the
//! services its runners hand every job (ADR-0034); a browser bootstraps no
//! store.
//!
//! Behind the `tools` feature, `StoreTools` is the hub's four tools for the agent: `check_in`, `list_stored`, `pin`,
//! and `unpin`.

mod check_in;
mod hash;
mod hold;
mod jobs;
mod launch;
mod list;
mod name;
mod pin;
mod record;
mod settings;
mod stockroom;
mod store;
#[cfg(feature = "tools")]
mod tools;

pub use check_in::{CheckIn, CheckedIn, StoredBinary};
pub use hash::StoreHash;
pub use hold::{Held, Hold};
pub use launch::{Binary, Launch, Launched};
pub use list::{Entry, ListStored, Stored};
pub use name::StoreName;
pub use pin::{Pin, Pinned, Unpin};
pub use settings::{StoreConfig, StoreSettings};
pub use stockroom::{
    Copied, Copying, Keeping, Launching, NamesFile, RecordFile, Recounting, Shelf, Stockroom, Uncopied, Unkept,
};
pub use store::{Store, Uncounted};
#[cfg(feature = "tools")]
pub use tools::{Asking, StoreTools, StoreToolsConfig};
