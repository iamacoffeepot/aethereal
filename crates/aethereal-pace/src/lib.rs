//! The program's clock (ADR-0045): the root [`Pacer`], which holds it or lets it run, and advances a held one by the
//! deltas it is asked for, each a cycle whose `Tick` carries that delta exactly.
//!
//! It names no host. The root asks its host through its controller, [`Pacing`], and hears through its source,
//! [`Paced`], that a held cycle has finished; every chassis keeps the same [`Clockwork`], which takes the root's
//! requests between steps and says when a held cycle is due and when it has finished. Only the program's time is
//! held: a step's time, which deadlines and every job's timer run on, is the host's own.
//!
//! The `tools` feature's `set_pace` and `advance` ask the pacer for an agent (ADR-0046).

mod bootstrap;
mod host;
mod pacer;
#[cfg(feature = "tools")]
mod tool;

pub use bootstrap::PaceSettings;
pub use host::Clockwork;
pub use pacer::{
    Advance, Advanced, Pace, PaceRead, PaceSet, Paced, Pacer, PacerConfig, Pacing, ReadPace, SetPace, Unadvanced,
};
#[cfg(feature = "tools")]
pub use tool::{Calling, PaceTool, PaceToolConfig};
