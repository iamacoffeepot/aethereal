//! The process capability on the native host: its launcher, each program started in a process group of its own and
//! waited on by threads of its own, and its bootstrap, which serves the launcher, and the host's poller that its pipes
//! wait in (ADR-0040, ADR-0033, ADR-0034).
//!
//! The launcher is built where the host has process groups and the C library
//! the headless chassis already links: Linux and Apple's systems. Elsewhere
//! the bootstrap serves none, and every start is refused.

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
mod groups;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
mod head_and_tail;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
pub use groups::ProcessGroups;

use crate::{Headless, Poller};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use aethereal_actor::Service;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use aethereal_process::Launcher;
use aethereal_process::{Processes, ProcessesSettings};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use std::sync::Arc;

/// The process capability on the native host: its launcher served to the host for its runners, and the host's poller,
/// which its pipes wait in as the TCP capability's connections do (ADR-0040, ADR-0033). A host with no system poller
/// serves no `Readiness`.
impl Bootstrap<Headless> for Processes {
    const SECTION: &'static str = "processes";
    type Settings = ProcessesSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: ProcessesSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
        cx.serve(Service::<dyn Launcher>::of(Arc::new(ProcessGroups::default())));
        if Poller::WATCHES_SOCKETS {
            let readiness = cx.host().readiness();
            cx.serve(readiness);
        }
        cx.begin(&(), settings.config())?;

        Ok(())
    }
}
