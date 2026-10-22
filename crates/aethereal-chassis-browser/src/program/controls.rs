//! The controls root bootstrapped on the browser: the page's controls listened to, and brought in through its source
//! (ADR-0024, ADR-0029).

use super::unopened;
use crate::controls::{self, Controls, ControlsConfig};
use crate::{Browser, Performance};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};

/// Begins the controls root with its source, and gives the browser the page's listeners on each control it marks,
/// which push what each holds once now and again each time it changes.
impl Bootstrap<Browser<Performance>> for Controls {
    const SECTION: &'static str = "controls";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Browser<Performance>, Self>) -> Result<(), Unbuilt> {
        let (source, feed) = cx.source();
        cx.begin(&(), ControlsConfig { source })?;
        cx.serve(controls::listen(&feed).map_err(unopened)?);

        Ok(())
    }
}
