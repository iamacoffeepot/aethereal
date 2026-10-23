//! The file capability bootstrapped on the desktop: the native host's directories, served to the host for its runners
//! (ADR-0029, ADR-0032).

use crate::running::Desktop;
use aethereal_chassis_headless::{Directories, FileRoots};
use aethereal_files::{FileHosting, Files, FilesConfig};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};

/// The file capability on the desktop, as on the headless host: its storage the directories its settings name.
impl Bootstrap<Desktop> for Files {
    const SECTION: &'static str = "files";
    type Settings = FileRoots;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(roots: FileRoots, (): (), cx: &mut Bootstrapping<'_, Desktop, Self>) -> Result<(), Unbuilt> {
        cx.serve(FileHosting::new(Directories::new(&roots)));
        cx.begin(&(), FilesConfig)
    }
}
