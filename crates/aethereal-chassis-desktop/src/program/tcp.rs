//! The TCP capability bootstrapped on the desktop: the native host's dialer, served into the services its runners hand
//! every job (ADR-0033, ADR-0034).

use crate::running::Desktop;
use aethereal_actor::Service;
use aethereal_chassis_headless::Dialers;
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_tcp::{Dialer, Tcp, TcpSettings};
use std::sync::Arc;

/// The TCP capability on the desktop, as on the headless host: its dials on threads of the host's own.
impl Bootstrap<Desktop> for Tcp {
    const SECTION: &'static str = "tcp";
    type Settings = TcpSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: TcpSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Dialer>::of(Arc::new(Dialers::of(&settings))));
        cx.begin(&(), settings.config())
    }
}
