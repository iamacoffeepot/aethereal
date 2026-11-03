//! The link's hub end bootstrapped on the desktop: the system's randomness, served to the host for its runners
//! (ADR-0029, ADR-0040).

use crate::running::Desktop;
use aethereal_actor::Service;
use aethereal_chassis_headless::SystemRandomness;
use aethereal_link::{Engines, EnginesSettings, Randomness};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use std::sync::Arc;

/// The link's hub end on the desktop, as on the headless host: its tokens minted from the system's randomness.
impl Bootstrap<Desktop> for Engines {
    const SECTION: &'static str = "engines";
    type Settings = EnginesSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: EnginesSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(), Unbuilt> {
        settings.check(cx.read_limit(), cx.blob_bytes())?;
        let randomness = SystemRandomness::open().map_err(|why| Unbuilt::Failed(Box::new(why)))?;
        cx.serve(Service::<dyn Randomness>::of(Arc::new(randomness)));

        cx.begin(&(), settings)
    }
}
