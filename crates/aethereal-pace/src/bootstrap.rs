//! The pacer bootstrapped on any host that keeps its clockwork: its settings, and how it is stood up (ADR-0029,
//! ADR-0045).

use crate::host::Clockwork;
use crate::pacer::{Pace, Paced, Pacer, PacerConfig, Pacing};
use aethereal_data::Plain;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Serves, Unbuilt};

/// The pacer's settings, its section, `pace` (ADR-0028).
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[kind(name = "aether.pace.settings")]
#[settings]
pub struct PaceSettings {
    /// How the program's clock moves as it starts: `real`, with the host's time; or `held`, only as it is advanced,
    /// so its first `Tick` is one an advance chose.
    #[flag]
    pub pace: Pace,
}

/// Begins the pacer with a source and a controller of its own, the clock moving as its settings say, the engine's
/// limit on a `Tick`'s delta the most it advances one by, and gives the host its clockwork. Every host bootstraps it
/// the same way, as each keeps the same clockwork.
impl<H: Host + Serves<Clockwork>> Bootstrap<H> for Pacer {
    const SECTION: &'static str = "pace";
    type Settings = PaceSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: PaceSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let source = cx.source::<Paced>();
        let controller = cx.controller::<Pacing>();
        let limit = cx.delta_limit();
        let (config, clockwork) = PacerConfig::paired(source, controller, settings.pace, limit);
        cx.begin(&(), config)?;
        cx.serve::<Clockwork>(clockwork);

        Ok(())
    }
}
