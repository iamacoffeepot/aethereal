//! An MCP server on this machine with two tools, `read_file` and `write_file`, for a client or a measure to reach
//! (ADR-0037, ADR-0039). It runs
//! no cycle: its sockets wake it as they are ready, and it sleeps between requests (ADR-0033).
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example mcp_server -- --port 8890 --assets DIR --save DIR`.

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::{Files, ReadTool, WriteTool};
use aethereal_http::{Http, HttpSettings};
use aethereal_mcp::Mcp;
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::Toolbox;

fn main() {
    let http = HttpSettings::default();
    let read_limit = u32::try_from(http.most_read_bytes()).unwrap_or(u32::MAX);
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root::<Mcp>()
        .root::<Files>()
        .root::<Toolbox<(ReadTool, WriteTool), Mcp>>()
        .launch("aethereal_mcp");
    assert!(launched.is_ok(), "the server runs: {launched:?}");
}
