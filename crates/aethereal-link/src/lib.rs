//! The link to engines (ADR-0038): an engine's tools, handed to its hub over TCP. The crate holds both ends of one
//! protocol, and a program lists the end it is.
//!
//! In an engine, [`Link`] is a tools [`Holder`]: any actor may [`Offer`] it tools at its typed path,
//! `path::<Link>(())`, as it would `Mcp` in a hub. It dials its hub as it begins, and dials again whenever the hub
//! goes, so nothing in the engine sees it go.
//!
//! In a hub, [`Engines`] listens for engines, and keeps an [`Engine`] beneath itself for each one linked, keyed by its
//! id. The hub's tools ask `Engines` to [`ListEngines`], and ask an engine at `path::<Engine>(id)` to
//! [`DescribeEngine`] or to [`CallEngine`].
//!
//! Behind the `tools` feature, `EngineTools` is the hub's three tools for the agent: `list_engines`,
//! `describe_engine`, and `call_tool`, which a hub lists in its toolbox after `Engines`. It converts JSON to an
//! engine's bytes with the sheets the engine sent, so the hub needs no kind of the engine's.
//!
//! Only tools cross the link: each frame is one plain kind's bytes in a `LengthPrefixed` frame of `Tcp`'s, an [`Up`]
//! from the engine or a [`Down`] from the hub. A connection opens with [`Up::Hello`], which names the [`REVISION`] the
//! engine speaks, and then [`Up::Sheets`], every tool the engine holds, which it writes again whole after every
//! change. Each set has a revision, and each [`Down::Call`] names the set the hub converted it with, so the engine
//! refuses a call of a set it has changed since.
//!
//! Each end needs nothing of its host but `Tcp`, so each is bootstrapped for every host. Their settings, the sections
//! `link` and `engines`, are their configs. A toolbox in an engine is listed after `Link`, and `Link` after `Tcp`:
//!
//! ```
//! # use aethereal_actor::Ordered;
//! # use aethereal_link::Link;
//! # use aethereal_tcp::Tcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Toolbox<(), Link>, (Link, (Tcp, ()))), _>();
//! ```
//!
//! Before it, the program does not build:
//!
//! ```compile_fail
//! # use aethereal_actor::Ordered;
//! # use aethereal_link::Link;
//! # use aethereal_tcp::Tcp;
//! # use aethereal_tools::Toolbox;
//! # fn ordered<R: Ordered<I>, I>() {}
//! ordered::<(Link, (Toolbox<(), Link>, (Tcp, ()))), _>();
//! ```
//!
//! [`Holder`]: aethereal_tools::Holder
//! [`Offer`]: aethereal_tools::Offer

mod engines;
mod frames;
mod link;
mod read_limit;
#[cfg(feature = "tools")]
mod tools;

pub use engines::{
    Binding, CallEngine, DescribeEngine, Described, Engine, EngineConfig, EngineList, EngineListing, Engines,
    EnginesSettings, ListEngines, Sent, Shown, Unlistened,
};
pub use frames::{Down, PORT, REVISION, Up};
pub use link::{Calling, Connecting, Link, LinkSettings, Sending, Woken};
pub use read_limit::ReadLimitTooLow;
#[cfg(feature = "tools")]
pub use tools::{Asking, EngineAnswer, EngineCall, EngineId, EngineSheets, EngineTools, EngineToolsConfig, JsonSheet};
