//! The hub: an MCP server whose tools list the engines linked to it, describe an engine's tools, call one, mint a token
//! for an engine started by hand, spawn an engine from its store, and terminate one (ADR-0037, ADR-0038, ADR-0040),
//! and check binaries into its store, list them, and pin them (ADR-0039). It lists `Processes`, which starts each
//! engine it spawns, and `actor_logs`, which reads what its own actors logged (ADR-0044). It runs the Claude sessions
//! its caller opens, each in a Docker container of its own, and offers them as the `session_*` tools (ADR-0057), and
//! puts each trusted engine's chat through to them with `ChatToSessions`. Its own tool `describe_hub` answers the
//! commit it was built from and the link revision it speaks (ADR-0038). It runs no cycle: its sockets wake it as they
//! are ready (ADR-0033).
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
//! `LocalStack`, is on the loopback, `local_stack.port`, 8890 unless set otherwise, and every request on it is the
//! owner's. The sessions', `ContainerStack`, is on `container_stack.port`, 8893, bound on `container_stack.interface`,
//! Docker's default bridge gateway unless set otherwise, where the sessions' containers reach it at
//! `sessions.hub_url`; `Sessions` checks the bearer each request there bears, one it minted for a session's container
//! (ADR-0057). Neither `Mcp` nor `Sessions` names the other: the stacks name both.
//!
//! Build with `cargo build --release -p aethereal-hub`, and run with `aethereal-hub --link-port 8892 --root store`. The
//! store is kept in the directory `--root` names. Then check an engine's binary in, such as the `link_engine` example's,
//! and spawn it with `spawn_engine`, or start one by hand with the arguments `mint_token` answers.
//!
//! It is deployed as the systemd user service `deploy/aethereal-hub.service`, installed unchanged, whose comments say
//! how; a test reads its command line as the hub does, so a change that moves a flag or key updates it.

mod describe;
mod stacks;
#[cfg(test)]
mod tests;

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::Files;
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{ChatToSessions, EngineTools, Engines, EnginesSettings};
use aethereal_mcp::{Mcp, McpSettings};
use aethereal_process::Processes;
use aethereal_sessions::{Docker, DockerSettings, Sessions, SessionsTools};
use aethereal_store::{Store, StoreSettings, StoreTools};
use aethereal_substrate::{EngineSettings, Host, Program};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox};
use describe::DescribeHubTool;
use stacks::{ContainerStack, LocalStack};

/// The program's name, which prefixes its environment's variables.
const NAME: &str = "aethereal_hub";

/// The hub's tools, offered through `Mcp`.
type HubTools = Toolbox<(EngineTools, StoreTools, LogsTool, SessionsTools, DescribeHubTool), Mcp>;

/// The hub's roots, the last listed first.
type Roots = (
    ContainerStack,
    (
        LocalStack,
        (
            HubTools,
            (ChatToSessions, (Sessions, (Docker, (Files, (Processes, (Store, (Engines, (Mcp, (Http, (Tcp, ())))))))))),
        ),
    ),
);

/// The hub: its roots, the engine's settings they need, and its flags named apart.
fn hub() -> Program<Headless, Roots> {
    let http = HttpSettings::default();
    let engines = EnginesSettings::default();
    let (local, container) = (stacks::local(), stacks::container());
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
        .root::<ChatToSessions>()
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
