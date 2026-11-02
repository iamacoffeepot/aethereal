//! The process capability bootstrapped on the desktop: the native host's launcher, served to the host for its runners
//! (ADR-0034, ADR-0040).

use crate::running::Desktop;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use aethereal_actor::Service;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use aethereal_chassis_headless::ProcessGroups;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use aethereal_process::Launcher;
use aethereal_process::{Processes, ProcessesSettings};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
use std::sync::Arc;

/// The process capability on the desktop, as on the headless host: each program in a process group of its own, where
/// the host has them, and every start refused where it has none.
impl Bootstrap<Desktop> for Processes {
    const SECTION: &'static str = "processes";
    type Settings = ProcessesSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: ProcessesSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(), Unbuilt> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
        cx.serve(Service::<dyn Launcher>::of(Arc::new(ProcessGroups::new(&settings))));
        cx.begin(&(), settings.config())
    }
}
