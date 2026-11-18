//! The sessions' stack (ADR-0035, ADR-0057): where the containers reach the hub, each request let in by the bearer it
//! bears.

use aethereal_actor::{Answer, Answered, Fault, Level, Receives, Turn};
use aethereal_data::Bound;
use aethereal_http::stack::{Binding, Listened, Serving, StackSettings};
use aethereal_mcp::{Mcp, McpRoutes};
use aethereal_sessions::Sessions;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{Interface, Listening};
use aethereal_tools::Caller;

/// The layers of the sessions' stack: `Sessions` says which session's bearer a request bears, or refuses it, and it
/// goes to `Mcp` on `POST /mcp`.
type ContainerLayers = (Sessions, McpRoutes);

/// The sessions' stack (ADR-0035, ADR-0057): where the containers reach the hub, each request let in by the bearer it
/// bears. A port it cannot listen on is logged, and it serves nothing: the owner's port is served without it.
#[derive(Bound)]
#[kind(name = "aether.hub.container_stack")]
pub struct ContainerStack {
    serving: Serving<Self, ContainerLayers>,
}

stack!(ContainerStack, ContainerLayers, [Caller], depends(Sessions, Mcp));

impl<H: Host> Bootstrap<H> for ContainerStack {
    const SECTION: &'static str = "container_stack";
    type Settings = StackSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: StackSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        settings.fit(cx)?;

        cx.begin(&(), settings)
    }
}

impl Receives<Answer<Listening, Binding>> for ContainerStack {
    /// With no port for the sessions, the owner's port is served without it, and the sessions reach none of the hub's
    /// tools.
    fn handle(&mut self, listening: (Answered<Listening>, Binding), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let listened = self.serving.listened(listening, turn)?;
        if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
            turn.log(
                Level::Error,
                format_args!("no port for the sessions, which reach none of the hub's tools: {listened:?}"),
            );
        }

        Ok(())
    }
}

/// The sessions' stack's defaults: 8893, on the gateway of Docker's default bridge, where its containers reach this
/// machine.
pub fn defaults() -> StackSettings {
    StackSettings { port: 8893, interface: Interface::At(String::from("172.17.0.1")), ..StackSettings::default() }
}
