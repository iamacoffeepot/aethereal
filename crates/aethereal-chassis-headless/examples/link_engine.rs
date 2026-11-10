//! An engine that links to its hub and hands it two tools: `read_file`, over the files of a directory (ADR-0038),
//! and `actor_logs`, which reads what its actors logged (ADR-0044). Its tools are held by `Tools`, and the link hands
//! them to the hub (ADR-0053). It dials the hub once, as it starts, with the token the hub minted for it, and ends when its link ends (ADR-0040).
//!
//! Its flags are the link's (`--hub`, `--port`, `--hub-token`, `--name`, `--most-bytes`) and the files' (`--assets`,
//! `--readers`); any field is also set by its section and name, `--set tools.most_tools=64`, and
//! by the environment, `AETHEREAL_ENGINE_ASSETS=DIR`. `--hub-token` is required: an engine started without it fails
//! as it begins.
//!
//! With a hub from the `link_hub` example running, ask its `mint_token` tool for a token, which answers the arguments
//! to start this engine with, then run `cargo run --release -p aethereal-chassis-headless --example link_engine --
//! <those arguments> --name scene --assets DIR`.

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::{Files, ReadTool};
use aethereal_link::{Link, LinkSettings};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox, Tools};

fn main() {
    let link = LinkSettings::default();
    let engine = EngineSettings { read_limit: link.most_bytes, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root::<Tools>()
        .root_with::<Link>(link)
        .root::<Files>()
        .root::<Toolbox<(ReadTool, LogsTool), Tools>>()
        .launch("aethereal_engine");
    assert!(launched.is_ok(), "the engine runs: {launched:?}");
}
