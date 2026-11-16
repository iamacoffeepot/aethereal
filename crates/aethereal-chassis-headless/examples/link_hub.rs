//! A hub on this machine: an MCP server whose tools list the engines linked to it, describe an engine's tools, call
//! one, mint a token for an engine started by hand, spawn an engine from its store, and terminate one (ADR-0037,
//! ADR-0038, ADR-0040), and check binaries into its store, list them, and pin them (ADR-0039). It lists `Processes`,
//! which starts each engine it spawns, and `actor_logs`, which reads what its own actors logged (ADR-0044). It runs the
//! Claude sessions its caller opens, each in a Docker container of its own, and offers them as the `session_*` tools
//! (ADR-0057). It runs no cycle: its sockets wake it as they are ready (ADR-0033).
//!
//! `Mcp` and `Engines` each suggest `--port` and `--most-calls`, `Engines` and `Store` each suggest `--most-bytes`, and
//! `Http` and `Docker` each suggest `--window`, so this program names them apart: `--mcp-port` and `--mcp-most-calls`
//! for MCP, `--link-port`, `--link-most-calls`, and `--link-most-bytes` for the engines' link, `--store-most-bytes`
//! for the store, and `--docker-window` for Docker's streams. Any
//! field is also set by its section and name, `--set engines.most_engines=8`, and by the environment,
//! `AETHEREAL_HUB_LINK_PORT=8892`. The sessions are set in the section `sessions`, such as `--set
//! sessions.credential=/srv/claude.env`, and Docker's socket in `docker`. The sessions keep their records and
//! transcripts through `Files`, in the directory `--save` names, and each session's home is a volume Docker keeps.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example link_hub -- --mcp-port 8890 --link-port 8892
//! --root store`. The store is kept in the directory `--root` names. Then check an engine's binary in, such as the
//! `link_engine` example's, and spawn it with `spawn_engine`, or start one by hand with the arguments `mint_token`
//! answers.

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::Files;
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{EngineTools, Engines, EnginesSettings};
use aethereal_mcp::{Mcp, McpSettings};
use aethereal_process::Processes;
use aethereal_sessions::{Docker, DockerSettings, Sessions, SessionsTools};
use aethereal_store::{Store, StoreSettings, StoreTools};
use aethereal_substrate::{EngineSettings, Host, Program};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox};

/// The program's name, which prefixes its environment's variables.
const NAME: &str = "aethereal_hub";

/// The hub's tools, offered through `Mcp`.
type HubTools = Toolbox<(EngineTools, StoreTools, LogsTool, SessionsTools), Mcp>;

/// The hub's roots, the last listed first.
type Roots = (HubTools, (Sessions, (Docker, (Files, (Processes, (Store, (Engines, (Mcp, (Http, (Tcp, ()))))))))));

/// The hub: its roots, the engine's settings they need, and its flags named apart.
fn hub() -> Program<Headless, Roots> {
    let http = HttpSettings::default();
    let engines = EnginesSettings::default();
    let read_limit = u32::try_from(http.most_read_bytes()).unwrap_or(u32::MAX).max(engines.most_bytes);
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
        .flags(|flags| {
            flags
                .rename(McpSettings::PORT, "mcp-port")
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
