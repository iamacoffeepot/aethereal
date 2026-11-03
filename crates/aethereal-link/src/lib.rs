//! The link to engines (ADR-0038, ADR-0040): an engine's tools, handed to its hub over TCP. The crate holds both ends
//! of one protocol, and a program lists the end it is.
//!
//! In an engine, [`Link`] is a tools [`Holder`]: any actor may [`Offer`] it tools at its typed path,
//! `path::<Link>(())`, as it would `Mcp` in a hub. It dials its hub once, as it begins, with the [`Token`] its hub
//! minted for it, its required setting `--hub-token`, and ends its program when the link ends: no engine outlives its
//! link.
//!
//! In a hub, [`Engines`] listens for engines, mints the tokens they link with, and keeps an [`Engine`] beneath itself
//! for each one linked, keyed by its id. A token is good for one `Hello`: an engine is listed only once `Engines` has
//! taken the token its `Hello` brought. It spawns engines from the hub's store too, a [`Spawned`] beneath itself for
//! each, which launches its binary from the `Store` and starts it through `Processes`, so a hub that spawns lists
//! both. The hub's tools ask `Engines` to [`ListEngines`], to [`MintToken`], to [`SpawnEngine`], or to
//! [`TerminateEngine`], and ask an engine at `path::<Engine>(id)` to [`DescribeEngine`] or to [`CallEngine`].
//!
//! Behind the `tools` feature, `EngineTools` is the hub's tools for the agent: `list_engines`, `describe_engine`,
//! `call_tool`, `mint_token`, `spawn_engine`, and `terminate_engine`, which a hub lists in its toolbox after `Engines`.
//! It converts JSON to an engine's bytes with the sheets the engine sent, so the hub needs no kind of the engine's.
//!
//! Only tools cross the link: each frame is one plain kind's bytes in a `LengthPrefixed` frame of `Tcp`'s, an [`Up`]
//! from the engine or a [`Down`] from the hub. A connection opens with [`Up::Hello`], which names the [`REVISION`] the
//! engine speaks and brings its token, and then [`Up::Sheets`], every tool the engine holds, which it writes again
//! whole after every change. Each set has a revision, and each [`Down::Call`] names the set the hub converted it
//! with, so the engine refuses a call of a set it has changed since. A call carries the time it has left, [`Within`],
//! before the deadline of the request it answers: `Engine` frees its slot when that time comes, and `Link` stops
//! waiting for its tool then (ADR-0041).
//!
//! `Link` needs nothing of its host but `Tcp`, so it is bootstrapped for every host. `Engines` needs the host's
//! [`Randomness`] too, which each native host's crate serves as it bootstraps it. Their settings, the sections `link`
//! and `engines`, are their configs. A toolbox in an engine is listed after `Link`, and `Link` after `Tcp`:
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
mod token;
#[cfg(feature = "tools")]
mod tools;

pub use engines::{
    Binding, Booting, CallEngine, DescribeEngine, Described, Engine, EngineConfig, EngineList, EngineListing, Engines,
    EnginesSettings, ExitedEngine, Fetching, Lapsing, LinkedAs, ListEngines, MintToken, Minted, Minting, Origin,
    Patience, Randomness, Runs, Sent, Shown, Spawn, SpawnEngine, Spawned, SpawnedConfig, TakeToken, TerminateEngine,
    Terminated, TokenFor, Unfilled, Unlistened, Vouching,
};
pub use frames::{Down, Outcome, PORT, REVISION, Refusal, Up, Within};
pub use link::{Calling, Connecting, Link, LinkConfig, LinkSettings, NoToken, Sending};
pub use read_limit::ReadLimitTooLow;
pub use token::Token;
#[cfg(feature = "tools")]
pub use tools::{
    Asking, EngineAnswer, EngineCall, EngineId, EngineSheets, EngineTools, EngineToolsConfig, JsonSheet, SpawnedEngine,
};
