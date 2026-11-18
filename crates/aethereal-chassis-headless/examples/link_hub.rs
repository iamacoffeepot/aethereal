//! A hub on this machine: an MCP server whose tools list the engines linked to it, describe an engine's tools, call
//! one, mint a token for an engine started by hand, spawn an engine from its store, and terminate one (ADR-0037,
//! ADR-0038, ADR-0040), and check binaries into its store, list them, and pin them (ADR-0039). It lists `Processes`,
//! which starts each engine it spawns, and `actor_logs`, which reads what its own actors logged (ADR-0044). It runs the
//! Claude sessions its caller opens, each in a Docker container of its own, and offers them as the `session_*` tools
//! (ADR-0057). It runs no cycle: its sockets wake it as they are ready (ADR-0033).
//!
//! `Mcp` and `Engines` each suggest `--most-calls`, `Engines` and `Store` each suggest `--most-bytes`, and `Http` and
//! `Docker` each suggest `--window`, so this program names them apart: `--mcp-most-calls` for MCP, `--link-port`,
//! `--link-most-calls`, and `--link-most-bytes` for the engines' link, `--store-most-bytes` for the store, and
//! `--docker-window` for Docker's streams. Any field is also set by its section and name, `--set
//! engines.most_engines=8`, and by the environment, `AETHEREAL_HUB_LINK_PORT=8892`. The sessions are set in the
//! section `sessions`, such as `--set sessions.credential=/srv/claude.env`, and Docker's socket in `docker`. The
//! sessions keep their records and transcripts through `Files`, in the directory `--save` names, and each session's
//! home is a volume Docker keeps.
//!
//! MCP is served by two stacks of the hub's own (ADR-0035), each its own section, which suggests no flags. The owner's,
//! [`LocalStack`], is on the loopback, `local_stack.port`, 8890 unless set otherwise, and every request on it is the
//! owner's. The sessions', [`ContainerStack`], is on `container_stack.port`, 8893, bound on
//! `container_stack.interface`, Docker's default bridge gateway unless set otherwise, where the sessions' containers
//! reach it at `sessions.hub_url`; `Sessions` checks the bearer each request there bears, one it minted for a session's
//! container (ADR-0057). Neither `Mcp` nor `Sessions` names the other: the stacks name both.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example link_hub -- --link-port 8892 --root store`.
//! The store is kept in the directory `--root` names. Then check an engine's binary in, such as the `link_engine`
//! example's, and spawn it with `spawn_engine`, or start one by hand with the arguments `mint_token` answers.

use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Level, Number, Part, Receives, Slept,
    Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_data::Bound;
use aethereal_files::Files;
use aethereal_http::stack::{
    Answering, Binding, Deadline, Gather, Inward, Layers, Listened, Outward, Parked, ParkedPart, ParkedResponse,
    PartStep, Response, Sending, Serving, Stack, StackSends, StackSettings, layers,
};
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{EngineTools, Engines, EnginesSettings};
use aethereal_mcp::{MarkOwner, Mcp, McpRoutes, McpSettings};
use aethereal_process::Processes;
use aethereal_sessions::{Docker, DockerSettings, Sessions, SessionsTools};
use aethereal_store::{Store, StoreSettings, StoreTools};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Program, Unbuilt};
use aethereal_tcp::{Accepting, HungUp, Interface, Listening, Reading, Shut, Tcp, Unbound, Wrote};
use aethereal_tools::{Caller, LogsTool, Toolbox};

/// The program's name, which prefixes its environment's variables.
const NAME: &str = "aethereal_hub";

/// The hub's tools, offered through `Mcp`.
type HubTools = Toolbox<(EngineTools, StoreTools, LogsTool, SessionsTools), Mcp>;

/// The layers of the owner's stack: every request is the owner's, and goes to `Mcp` on `POST /mcp`.
type LocalLayers = (MarkOwner, McpRoutes);

/// The layers of the sessions' stack: `Sessions` says which session's bearer a request bears, or refuses it, and it
/// goes to `Mcp` on `POST /mcp`.
type ContainerLayers = (Sessions, McpRoutes);

/// The hub's roots, the last listed first.
type Roots = (
    ContainerStack,
    (LocalStack, (HubTools, (Sessions, (Docker, (Files, (Processes, (Store, (Engines, (Mcp, (Http, (Tcp, ()))))))))))),
);

/// The owner's stack (ADR-0035): on the loopback, whatever its section names, so only this machine's programs reach it,
/// and every request on it is the owner's. A port it cannot listen on stops the hub as it boots.
#[derive(Bound)]
#[kind(name = "aether.hub.local_stack")]
struct LocalStack {
    serving: Serving<Self, LocalLayers>,
}

/// The sessions' stack (ADR-0035, ADR-0057): where the containers reach the hub, each request let in by the bearer it
/// bears. A port it cannot listen on is logged, and it serves nothing: the owner's port is served without it.
#[derive(Bound)]
#[kind(name = "aether.hub.container_stack")]
struct ContainerStack {
    serving: Serving<Self, ContainerLayers>,
}

/// The stack `$stack`, whose layers are `$layers` and whose actor layers pass on each of `$inward`: a root, which
/// begins by finding its layers and listening, and each of its handlers of HTTP's and TCP's kinds, and of its actor
/// layers' answers, one call into its `Serving`; but its listen's answer, which each stack decides on itself.
macro_rules! stack {
    ($stack:ident, $layers:ty, [$($inward:ty),*]) => {
        impl ChildOf for $stack {
            type Parent = Top;
        }

        impl Actor for $stack {
            type Config = StackSettings;
            type Key = ();
            type Handles = (
                Answer<Listening, Binding>,
                Part<Accepting>,
                Answer<Unbound, Number>,
                Part<Reading>,
                Answer<HungUp, Number>,
                Answer<Wrote, Sending>,
                Answer<Response, Answering>,
                Answer<Slept, Deadline>,
                Gather,
                Shut,
                Answer<Outward, ParkedResponse>,
                Answer<PartStep, ParkedPart>,
                $(Answer<Inward<$inward>, Parked>,)*
            );
            type BeginSends = StackSends<$layers>;
            type EndSends = ();

            fn begin(settings: StackSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
                let layers: Layers<$layers> = layers(turn)?;

                Ok(Self { serving: Serving::listen(turn, &settings, layers)? })
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Stack for $stack {}

        impl Handles<Part<Accepting>> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.accept(part, turn)
            }
        }

        impl Receives<Answer<Unbound, Number>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                unbound: (Answered<Unbound>, Number),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.unbound(unbound, turn)
            }
        }

        impl Handles<Part<Reading>> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.read(part, turn)
            }
        }

        impl Receives<Answer<HungUp, Number>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                hung_up: (Answered<HungUp>, Number),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.hung_up(hung_up, turn)
            }
        }

        impl Receives<Answer<Wrote, Sending>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                wrote: (Answered<Wrote>, Sending),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.wrote(wrote, turn)
            }
        }

        impl Receives<Answer<Response, Answering>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                answered: (Answered<Response>, Answering),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.answered(answered, turn)
            }
        }

        impl Receives<Answer<Slept, Deadline>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                slept: (Answered<Slept>, Deadline),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.slept(slept, turn)
            }
        }

        impl Handles<Gather> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, gather: Gather, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.gather(gather, turn)
            }
        }

        impl Handles<Shut> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.shut(turn)
            }
        }

        impl Receives<Answer<Outward, ParkedResponse>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                returned: (Answered<Outward>, ParkedResponse),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.returned(returned, turn)
            }
        }

        impl Receives<Answer<PartStep, ParkedPart>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                passed: (Answered<PartStep>, ParkedPart),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.passed(passed, turn)
            }
        }

        $(
            impl Receives<Answer<Inward<$inward>, Parked>> for $stack {
                type Sends = StackSends<$layers>;

                fn handle(
                    &mut self,
                    entered: (Answered<Inward<$inward>>, Parked),
                    turn: &mut Turn<'_, Self, Self::Sends>,
                ) -> Result<(), Fault> {
                    self.serving.entered(entered, turn)
                }
            }
        )*
    };
}

stack!(LocalStack, LocalLayers, []);
stack!(ContainerStack, ContainerLayers, [Caller]);

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

impl Receives<Answer<Listening, Binding>> for LocalStack {
    type Sends = StackSends<LocalLayers>;

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

impl Receives<Answer<Listening, Binding>> for ContainerStack {
    type Sends = StackSends<ContainerLayers>;

    /// With no port for the sessions, the owner's port is served without it, and the sessions reach none of the hub's
    /// tools.
    fn handle(
        &mut self,
        listening: (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
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

/// The owner's stack's defaults: 8890, on the loopback.
fn local_stack() -> StackSettings {
    StackSettings { port: 8890, interface: Interface::Loopback, ..StackSettings::default() }
}

/// The sessions' stack's defaults: 8893, on the gateway of Docker's default bridge, where its containers reach this
/// machine.
fn container_stack() -> StackSettings {
    StackSettings { port: 8893, interface: Interface::At(String::from("172.17.0.1")), ..StackSettings::default() }
}

/// The hub: its roots, the engine's settings they need, and its flags named apart.
fn hub() -> Program<Headless, Roots> {
    let http = HttpSettings::default();
    let engines = EnginesSettings::default();
    let (local, container) = (local_stack(), container_stack());
    let heads = http.most_read_bytes().max(local.most_read_bytes()).max(container.most_read_bytes());
    let read_limit = u32::try_from(heads).unwrap_or(u32::MAX).max(engines.most_bytes);
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() };

    Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root::<Mcp>()
        .root_with::<Engines>(engines)
        .root::<Store>()
        .root::<Processes>()
        .root::<Files>()
        .root::<Docker>()
        .root::<Sessions>()
        .root::<HubTools>()
        .root_with::<LocalStack>(local)
        .root_with::<ContainerStack>(container)
        .flags(|flags| {
            flags
                .rename(McpSettings::MOST_CALLS, "mcp-most-calls")
                .rename(EnginesSettings::PORT, "link-port")
                .rename(EnginesSettings::MOST_CALLS, "link-most-calls")
                .rename(EnginesSettings::MOST_BYTES, "link-most-bytes")
                .rename(StoreSettings::MOST_BYTES, "store-most-bytes")
                .rename(DockerSettings::WINDOW, "docker-window")
        })
}

fn main() {
    let launched = hub().launch(NAME);
    assert!(launched.is_ok(), "the hub runs: {launched:?}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_chassis_headless::native::{Invocation, Invoked};

    #[test]
    fn the_hub_reads_its_sections_and_flags_with_no_two_of_one_name() {
        // Catches a root listed here whose settings suggest a flag another listed root already suggests, left so:
        // the hub would refuse to start, as it did when `Http` and `Docker` each suggested `--window`.
        let invocation = Invocation::new([NAME], Vec::<(String, String)>::new());

        let read = hub().read(NAME, invocation);

        let runs = matches!(read, Ok(Invoked::Run(_)));
        assert!(runs, "the hub reads an invocation of no arguments: {:?}", read.err());
    }
}
