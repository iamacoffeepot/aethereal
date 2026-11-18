//! An MCP server on this machine with two tools, `read_file` and `write_file`, for a client or a measure to reach
//! (ADR-0037, ADR-0039). It serves MCP through a stack of its own on the loopback (ADR-0035), every request on it the
//! owner's. It runs no cycle: its sockets wake it as they are ready, and it sleeps between requests (ADR-0033).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example mcp_server -- --set mcp_stack.port=8890
//! --assets DIR --save DIR`.

use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Level, Number, Part, Receives, Slept,
    Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_data::Bound;
use aethereal_files::{Files, ReadTool, WriteTool};
use aethereal_http::stack::{
    Answering, Binding, Deadline, Gather, Layers, Listened, Outward, ParkedPart, ParkedResponse, PartStep, Response,
    Sending, Serving, Stack, StackSends, StackSettings, layers,
};
use aethereal_mcp::{MarkOwner, Mcp, McpRoutes};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Accepting, HungUp, Interface, Listening, Reading, Shut, Tcp, Unbound, Wrote};
use aethereal_tools::Toolbox;

/// The stack's layers: every request is the owner's, and goes to `Mcp` on `POST /mcp`.
type McpLayers = (MarkOwner, McpRoutes);

/// The server's stack (ADR-0035): on the loopback, whatever its section, `mcp_stack`, names, so only this machine's
/// programs reach it, and every request on it is the owner's. A port it cannot listen on stops the server as it boots.
#[derive(Bound)]
#[kind(name = "aether.mcp_server.mcp_stack")]
struct McpStack {
    serving: Serving<Self, McpLayers>,
}

impl ChildOf for McpStack {
    type Parent = Top;
}

impl<H: Host> Bootstrap<H> for McpStack {
    const SECTION: &'static str = "mcp_stack";
    type Settings = StackSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: StackSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let settings = StackSettings { interface: Interface::Loopback, ..settings };
        settings.fit(cx)?;

        cx.begin(&(), settings)
    }
}

impl Actor for McpStack {
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
    );
    type BeginSends = StackSends<McpLayers>;
    type EndSends = ();

    fn begin(settings: StackSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let layers: Layers<McpLayers> = layers(turn)?;

        Ok(Self { serving: Serving::listen(turn, &settings, layers)? })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Stack for McpStack {}

impl Receives<Answer<Listening, Binding>> for McpStack {
    type Sends = StackSends<McpLayers>;

    /// With no port, the server has nothing to serve: it quits, and as its listen rides the boot, it does not start
    /// (ADR-0020).
    fn handle(
        &mut self,
        listening: (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let listened = self.serving.listened(listening, turn)?;
        if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
            turn.log(Level::Error, format_args!("no port to serve on, so the server stops: {listened:?}"));
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Part<Accepting>> for McpStack {
    type Sends = StackSends<McpLayers>;
    type From = Anyone;

    fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.serving.accept(part, turn)
    }
}

impl Receives<Answer<Unbound, Number>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        unbound: (Answered<Unbound>, Number),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.unbound(unbound, turn)
    }
}

impl Handles<Part<Reading>> for McpStack {
    type Sends = StackSends<McpLayers>;
    type From = Anyone;

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.serving.read(part, turn)
    }
}

impl Receives<Answer<HungUp, Number>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        hung_up: (Answered<HungUp>, Number),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.hung_up(hung_up, turn)
    }
}

impl Receives<Answer<Wrote, Sending>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        wrote: (Answered<Wrote>, Sending),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.wrote(wrote, turn)
    }
}

impl Receives<Answer<Response, Answering>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        answered: (Answered<Response>, Answering),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.answered(answered, turn)
    }
}

impl Receives<Answer<Slept, Deadline>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        slept: (Answered<Slept>, Deadline),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.slept(slept, turn)
    }
}

impl Handles<Gather> for McpStack {
    type Sends = StackSends<McpLayers>;
    type From = Anyone;

    fn handle(&mut self, gather: Gather, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.serving.gather(gather, turn)
    }
}

impl Handles<Shut> for McpStack {
    type Sends = StackSends<McpLayers>;
    type From = Anyone;

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.serving.shut(turn)
    }
}

impl Receives<Answer<Outward, ParkedResponse>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        returned: (Answered<Outward>, ParkedResponse),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.returned(returned, turn)
    }
}

impl Receives<Answer<PartStep, ParkedPart>> for McpStack {
    type Sends = StackSends<McpLayers>;

    fn handle(
        &mut self,
        passed: (Answered<PartStep>, ParkedPart),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.serving.passed(passed, turn)
    }
}

fn main() {
    let stack = StackSettings { port: 8890, interface: Interface::Loopback, ..StackSettings::default() };
    let read_limit = u32::try_from(stack.most_read_bytes()).unwrap_or(u32::MAX);
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root::<Mcp>()
        .root::<Files>()
        .root::<Toolbox<(ReadTool, WriteTool), Mcp>>()
        .root_with::<McpStack>(stack)
        .launch("aethereal_mcp");
    assert!(launched.is_ok(), "the server runs: {launched:?}");
}
