//! The file capability bootstrapped on the browser: the program's assets fetched relative to the page, served to the
//! host for its runner (ADR-0029, ADR-0032).

use crate::files::{Fetching, FileUrls};
use crate::{Browser, Performance};
use aethereal_files::{FileHosting, Files, FilesConfig};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};

/// Begins the file capability, and gives the browser its storage: fetches beneath the URLs its settings name.
impl Bootstrap<Browser<Performance>> for Files {
    const SECTION: &'static str = "files";
    type Settings = FileUrls;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        urls: FileUrls,
        (): (),
        cx: &mut Bootstrapping<'_, Browser<Performance>, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(FileHosting::new(Fetching::new(&urls)));
        cx.begin(&(), FilesConfig)
    }
}
