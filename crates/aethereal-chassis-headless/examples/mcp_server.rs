//! An MCP server on this machine with two tools, `read_file` and `write_file`, for a client or a measure to reach
//! (ADR-0037, ADR-0039). It serves MCP through a stack of its own on the loopback (ADR-0035), every request on it the
//! owner's. It runs no cycle: its sockets wake it as they are ready, and it sleeps between requests (ADR-0033).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example mcp_server -- --set mcp_stack.port=8890
//! --assets DIR --save DIR`.

use aethereal_actor::{
    Answered, Beginning, Ended, Enough, Fault, Later, Level, More, Number, Open, Part, Reply, Slept, Turn, actor,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_data::Bound;
use aethereal_files::{Files, ReadTool, WriteTool};
use aethereal_http::stack::{
    Answering, Binding, BodyRead, Deadline, Gather, Gathered, InboundBody, Layers, Listened, OutboundBody, Outward,
    ParkedPart, ParkedResponse, PartStep, Response, Sending, Serving, SourceEnd, Stack, StackSettings, layers,
};
use aethereal_mcp::{MarkOwner, Mcp, McpRoutes};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Accepting, HungUp, Interface, Listening, Reading, Shut, Tcp, Unbound, Wrote};
use aethereal_tools::Toolbox;
use std::process::ExitCode;

/// The stack's layers: every request is the owner's, and goes to `Mcp` on `POST /mcp`.
type McpLayers = (MarkOwner, McpRoutes);

/// The server's stack (ADR-0035): on the loopback, whatever its section, `mcp_stack`, names, so only this machine's
/// programs reach it, and every request on it is the owner's. A port it cannot listen on stops the server as it boots.
#[derive(Bound)]
#[kind(name = "aether.mcp_server.mcp_stack")]
struct McpStack {
    serving: Serving<Self, McpLayers>,
}

impl<H: Host> Bootstrap<H> for McpStack {
    const SECTION: &'static str = "mcp_stack";
    type Settings = StackSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: StackSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let settings = StackSettings { interface: Interface::Loopback, ..settings };
        settings.fit(cx)?;

        cx.begin(&(), settings)?;

        Ok(())
    }
}

#[actor(depends(Tcp, Mcp))]
impl McpStack {
    fn begin(settings: StackSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let layers: Layers<McpLayers> = layers(turn)?;

        Ok(Self { serving: Serving::listen(turn, &settings, layers)? })
    }

    /// With no port, the server has nothing to serve: it quits, and as its listen rides the boot, it does not start
    /// (ADR-0020).
    #[answer]
    fn listening(&mut self, listening: (Answered<Listening>, Binding), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let listened = self.serving.listened(listening, turn)?;
        if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
            turn.log(Level::Error, format_args!("no port to serve on, so the server stops: {listened:?}"));
            turn.quit();
        }

        Ok(())
    }

    #[tell]
    fn accepting(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.accept(part, turn)
    }

    #[answer]
    fn unbound(&mut self, unbound: (Answered<Unbound>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.unbound(unbound, turn)
    }

    #[tell]
    fn reading(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.read(part, turn)
    }

    #[answer]
    fn hung_up(&mut self, hung_up: (Answered<HungUp>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.hung_up(hung_up, turn)
    }

    #[answer]
    fn wrote(&mut self, wrote: (Answered<Wrote>, Sending), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.wrote(wrote, turn)
    }

    #[answer]
    fn response(&mut self, answered: (Answered<Response>, Answering), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.answered(answered, turn)
    }

    #[answer]
    fn slept(&mut self, slept: (Answered<Slept>, Deadline), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.slept(slept, turn)
    }

    #[request]
    fn gather(&mut self, gather: Gather, turn: &mut Turn<'_, Self>) -> Result<Reply<Gathered>, Fault> {
        self.serving.gather(gather, turn)
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.shut(turn)
    }

    #[answer]
    fn outward(
        &mut self,
        returned: (Answered<Outward>, ParkedResponse),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.serving.returned(returned, turn)
    }

    #[answer]
    fn part_step(&mut self, passed: (Answered<PartStep>, ParkedPart), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.passed(passed, turn)
    }

    #[request]
    fn open(&mut self, open: Open<InboundBody>, turn: &mut Turn<'_, Self>) -> Result<Later<BodyRead>, Fault> {
        self.serving.open_body(open, turn)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.grant_body(&more, turn)
    }

    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.stop_reading(&enough, turn)
    }

    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.lose_reader(&ended, turn)
    }

    #[tell]
    fn outbound_body(&mut self, part: Part<OutboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.take_part(part, turn)
    }

    #[answer]
    fn source_end(&mut self, ended: (Answered<SourceEnd>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.serving.end_source(ended, turn)
    }
}

impl Stack for McpStack {}

#[allow(clippy::print_stderr)] // How the server ended is written for whoever started it.
fn main() -> ExitCode {
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
    match launched {
        Ok(Launched::Ran(exit)) => {
            eprintln!("exit={exit:?}");
            exit.status()
        }
        Ok(Launched::Printed) => ExitCode::SUCCESS,
        Err(failed) => panic!("the server runs: {failed:?}"),
    }
}
