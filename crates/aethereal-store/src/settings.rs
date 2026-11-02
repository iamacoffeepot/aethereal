//! The store's settings (ADR-0028, ADR-0039).

use aethereal_data::Plain;

/// The store's section, `store`: where its host keeps it, and its budget (ADR-0039).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.store.settings")]
#[settings]
pub struct StoreSettings {
    /// The directory the store is kept in: relative to where the program runs, or absolute.
    #[flag]
    pub root: String,
    /// The most bytes the store holds, past which a check-in prunes.
    #[flag]
    pub most_bytes: u64,
    /// The most bytes one file checked in holds.
    #[flag]
    pub most_file_bytes: u64,
}

impl Default for StoreSettings {
    fn default() -> Self {
        Self { root: String::from("store"), most_bytes: 16 << 30, most_file_bytes: 1 << 30 }
    }
}

impl StoreSettings {
    /// What the root is made from: the settings that are its, and not its host's.
    #[must_use]
    pub const fn config(&self) -> StoreConfig {
        StoreConfig { most_bytes: self.most_bytes, most_file_bytes: self.most_file_bytes }
    }
}

/// What the store's root is made from: its budget. Where it is kept is its host's.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
#[kind(name = "aether.store.config")]
pub struct StoreConfig {
    /// The most bytes the store holds.
    pub most_bytes: u64,
    /// The most bytes one file checked in holds.
    pub most_file_bytes: u64,
}
