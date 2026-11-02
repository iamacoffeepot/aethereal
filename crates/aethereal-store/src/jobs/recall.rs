//! The job that removes launch copies, once their asker has ended (ADR-0040).

use crate::Stockroom;
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Removes the launch copies `copies` names. Private to this crate.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.recall")]
pub struct Recall {
    /// The launch copies' file names.
    pub copies: Vec<String>,
}

/// What the store keeps for a removal it runs: nothing, as it runs one job at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.recalling")]
pub struct Recalling;

/// What became of a [`Recall`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.recalled")]
pub enum Recalled {
    /// Every launch copy asked is gone.
    Done,
    /// The host serves no stockroom, or could not remove all of them.
    Failed,
}

impl Job for Recall {
    type Done = Recalled;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Recalled, Fault> {
        let Some(room) = job.service::<dyn Stockroom>() else {
            return Ok(Recalled::Failed);
        };
        let recalled = match room.unlaunch(self.copies).await {
            Ok(()) => Recalled::Done,
            Err(_) => Recalled::Failed,
        };

        Ok(recalled)
    }
}
