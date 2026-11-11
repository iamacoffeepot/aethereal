//! The program's code (ADR-0054): one root, [`Library`], which holds the engine's one publisher and says which blob
//! each kind's head is.
//!
//! As the program starts it lists `Code` through `Files`, reads every module directly in it, and publishes the head of
//! every kind each module makes. A system that owns guests ships its modules in its own code and asks the library to
//! hold them with [`Defaults`]: a default is published only for a kind that has no head, so it never replaces code
//! in `Code`, or code published since. While the program runs, [`PublishModule`] publishes a module of `Code` again,
//! moving the heads of the kinds it makes.
//!
//! The `tools` feature's `publish` asks the library to publish for an agent, and answers at once; its `upgrades` reads,
//! when the agent asks, who has moved onto each kind's head and who has not.

mod bootstrap;
mod defaults;
mod library;
mod publish;
#[cfg(feature = "tools")]
mod tool;

pub use defaults::{Defaulted, Defaults};
pub use library::{Library, LibraryConfig};
pub use publish::{ModulePublished, Numbered, PublishModule, Unloaded};
#[cfg(feature = "tools")]
pub use tool::{
    BehindGuest, Calling, GuestStanding, HeadRecord, LibraryTool, LibraryToolConfig, Publish, PublishRecord,
    ReadUpgrades, Upgrading,
};
