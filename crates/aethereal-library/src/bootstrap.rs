//! The library bootstrapped on any host: handed the engine's one publisher, with nothing to set (ADR-0029, ADR-0054).

use crate::{Library, LibraryConfig};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};

/// Begins the library with the program's one publisher, which the chassis makes once as the program starts. It reads
/// `Code` through `Files`, so every host bootstraps it the same way, listed after `Files`.
impl<H: Host> Bootstrap<H> for Library {
    const SECTION: &'static str = "library";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let publisher = cx.publisher()?;

        cx.begin(&(), LibraryConfig { publisher })
    }
}
