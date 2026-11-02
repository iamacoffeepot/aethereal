//! A hub on this machine: an MCP server whose tools list the engines linked to it, describe an engine's tools, and
//! call one (ADR-0037, ADR-0038), and check binaries into its store, list them, and pin them (ADR-0039). It runs no
//! cycle: its sockets wake it as they are ready (ADR-0033).
//!
//! `Mcp` and `Engines` each suggest `--port` and `--most-calls`, and `Engines` and `Store` each suggest
//! `--most-bytes`, so this program names them apart: `--mcp-port` and `--mcp-most-calls` for MCP, `--link-port`,
//! `--link-most-calls`, and `--link-most-bytes` for the engines' link, and `--store-most-bytes` for the store. Any
//! field is also set by its section and name, `--set engines.most_engines=8`, and by the environment,
//! `AETHEREAL_HUB_LINK_PORT=8892`.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example link_hub -- --mcp-port 8890 --link-port 8892
//! --root store`, then an engine with the `link_engine` example. The store is kept in the directory `--root` names.

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{EngineTools, Engines, EnginesSettings};
use aethereal_mcp::{Mcp, McpSettings};
use aethereal_store::{Store, StoreSettings, StoreTools};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::Toolbox;

fn main() {
    let http = HttpSettings::default();
    let engines = EnginesSettings::default();
    let read_limit = u32::try_from(http.most_read_bytes()).unwrap_or(u32::MAX).max(engines.most_bytes);
    let engine = EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root::<Mcp>()
        .root_with::<Engines>(engines)
        .root::<Store>()
        .root::<Toolbox<(EngineTools, StoreTools), Mcp>>()
        .flags(|flags| {
            flags
                .rename(McpSettings::PORT, "mcp-port")
                .rename(McpSettings::MOST_CALLS, "mcp-most-calls")
                .rename(EnginesSettings::PORT, "link-port")
                .rename(EnginesSettings::MOST_CALLS, "link-most-calls")
                .rename(EnginesSettings::MOST_BYTES, "link-most-bytes")
                .rename(StoreSettings::MOST_BYTES, "store-most-bytes")
        })
        .launch("aethereal_hub");
    assert!(launched.is_ok(), "the hub runs: {launched:?}");
}
