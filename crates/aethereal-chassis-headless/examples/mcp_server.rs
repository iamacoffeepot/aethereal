//! An MCP server on this machine with one tool, `read_file`, for a client or a measure to reach (ADR-0037).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example mcp_server -- --port 8890 --assets DIR`.

use aethereal_actor::Tick;
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::{Files, ReadTool};
use aethereal_http::{Http, HttpSettings};
use aethereal_mcp::{Mcp, Toolbox};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;

fn main() {
    let http = HttpSettings::default();
    let read_limit = u32::try_from(http.most_read_bytes()).unwrap_or(u32::MAX);
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .with_cycle::<(Tick,)>()
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root::<Mcp>()
        .root::<Files>()
        .root::<Toolbox<(ReadTool,)>>()
        .launch("aethereal_mcp");
    assert!(launched.is_ok(), "the server runs: {launched:?}");
}
