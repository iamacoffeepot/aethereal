//! The store bootstrapped on the desktop: the native host's stockroom, served to the host for its runners (ADR-0029,
//! ADR-0039).

use crate::running::Desktop;
use aethereal_actor::Service;
use aethereal_chassis_headless::StoreDirectory;
use aethereal_store::{Stockroom, Store, StoreSettings};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use std::sync::Arc;

/// The store on the desktop, as on the headless host: its stockroom the directory its settings name.
impl Bootstrap<Desktop> for Store {
    const SECTION: &'static str = "store";
    type Settings = StoreSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: StoreSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Stockroom>::of(Arc::new(StoreDirectory::new(&settings))));
        cx.begin(&(), settings.config())
    }
}
