//! The owner's stack (ADR-0035): on the loopback, so only this machine's programs reach it, and every request on it is
//! the owner's.

use aethereal_actor::{Answer, Answered, Fault, Level, Receives, Turn};
use aethereal_data::Bound;
use aethereal_http::stack::{Binding, ListenSends, Listened, Serving, StackSettings};
use aethereal_mcp::{MarkOwner, McpRoutes};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Interface, Listening};

/// The layers of the owner's stack: every request is the owner's, and goes to `Mcp` on `POST /mcp`.
type LocalLayers = (MarkOwner, McpRoutes);

/// The owner's stack (ADR-0035): on the loopback, whatever its section names, so only this machine's programs reach it,
/// and every request on it is the owner's. A port it cannot listen on stops the hub as it boots.
#[derive(Bound)]
#[kind(name = "aether.hub.local_stack")]
pub struct LocalStack {
    serving: Serving<Self, LocalLayers>,
}

stack!(LocalStack, LocalLayers, []);

/// Its section, `local_stack`, is bound on the loopback whatever it names, as `MarkOwner` takes every request on the
/// port for the owner's.
impl<H: Host> Bootstrap<H> for LocalStack {
    const SECTION: &'static str = "local_stack";
    type Settings = StackSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: StackSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let settings = StackSettings { interface: Interface::Loopback, ..settings };
        settings.fit(cx)?;

        cx.begin(&(), settings)
    }
}

impl Receives<Answer<Listening, Binding>> for LocalStack {
    type Sends = ListenSends;

    /// With no port for the owner, the hub has nothing to serve: it quits, and as its listen rides the boot, the hub
    /// does not start (ADR-0020).
    fn handle(
        &mut self,
        listening: (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let listened = self.serving.listened(listening, turn)?;
        if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
            turn.log(Level::Error, format_args!("no port for the owner, so the hub stops: {listened:?}"));
            turn.quit();
        }

        Ok(())
    }
}

/// The owner's stack's defaults: 8890, on the loopback.
pub fn defaults() -> StackSettings {
    StackSettings { port: 8890, interface: Interface::Loopback, ..StackSettings::default() }
}
