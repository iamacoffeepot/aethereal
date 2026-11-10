//! The program's resident memory, as the native host asks its operating system for it, for the engine's figures
//! (ADR-0053): each system's way of asking in a module of its own, and an unknown figure where the host has none.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;

use aethereal_actor::Process;

/// The program's resident memory, as the operating system reports it now: unknown where the host has no way to ask,
/// or the asking failed.
#[must_use]
pub fn resident() -> Process {
    #[cfg(target_os = "linux")]
    let bytes = linux::resident_bytes();
    #[cfg(target_os = "macos")]
    let bytes = macos::resident_bytes();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let bytes: Option<u64> = None;

    bytes.map_or(Process::Unknown, |bytes| Process::Resident { bytes })
}
