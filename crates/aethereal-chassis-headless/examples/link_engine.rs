//! An engine that links to its hub and hands it one tool, `read_file`, over the files of a directory (ADR-0038). It
//! dials the hub as it starts, and again whenever the hub goes.
//!
//! Its flags are the link's (`--hub`, `--port`, `--name`, `--most-tools`, `--most-bytes`) and the files' (`--assets`,
//! `--readers`); any field is also set by its section and name, `--set link.redial_millis=[100,5000]`, and by the
//! environment, `AETHEREAL_ENGINE_ASSETS=DIR`.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example link_engine -- --port 8892 --name scene
//! --assets DIR`, with a hub from the `link_hub` example listening on that port.

use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::NativeLayers;
use aethereal_files::{Files, ReadTool};
use aethereal_link::{Link, LinkSettings};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::Toolbox;

fn main() {
    let link = LinkSettings::default();
    let engine = EngineSettings { read_limit: link.most_bytes, mail_bytes: 1 << 24, ..EngineSettings::default() };
    let launched = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Link>(link)
        .root::<Files>()
        .root::<Toolbox<(ReadTool,), Link>>()
        .launch("aethereal_engine");
    assert!(launched.is_ok(), "the engine runs: {launched:?}");
}
