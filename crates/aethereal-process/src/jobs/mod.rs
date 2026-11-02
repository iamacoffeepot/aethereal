//! The jobs only this crate makes: each the launcher's work, run on `Io` as it waits on the host (ADR-0018, ADR-0040).
//! The root alone launches a program; its process alone waits on it and signals it.

mod launch;
mod raise;
mod wait;

pub use launch::{Launch, Launched, Launching};
pub use raise::{Raise, Raised, Raising};
pub use wait::{Wait, Waited, Waiting};
