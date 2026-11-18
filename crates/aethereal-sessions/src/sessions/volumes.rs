//! A session's volumes (ADR-0057): its home, its cargo target directory, and the cargo home every session of the hub
//! shares, each a named volume Docker keeps where it keeps volumes, so the hub never names a path of this machine for
//! one. Before each container of a session, the hub asks Docker to make each, labelled, one at a time, which Docker
//! answers for a volume already made as for a new one. The session image holds the directory each is mounted at,
//! owned by the session's user, and Docker copies that directory, its owner with it, into a new, empty volume as the
//! container is made, so every volume is that user's from its first container. Archiving a session keeps them.

use super::Sessions;
use super::asks::Refusal;
use super::container;
use super::credential::{Crediting, Provision};
use super::session::{Life, Step};
use crate::{Docker, DockerError, Unwritable, VolumeCreated};
use aethereal_actor::{Answer, Answered, Fault, Receives, Turn, path};
use aethereal_data::Plain;

/// What the capability keeps for a request of Docker that makes a volume of a session's: the session, and the run it
/// is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.homing")]
pub struct Homing {
    pub session: u32,
    pub run: u32,
}

impl Sessions {
    /// Asks Docker to make the volume numbered `made`, from 0, of those `session`'s container mounts, for its run
    /// `run`; once each is made, the credential is read.
    pub(super) fn house(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32, made: u32) -> Result<(), Fault> {
        let Ok(volumes) = container::volumes(&self.settings, session) else {
            return self.unmade(turn, session, run);
        };
        let at = usize::try_from(made).unwrap_or(usize::MAX);
        let Some(create) = volumes.into_iter().nth(at) else {
            turn.run(&Provision { session }, &Crediting { session, run })?;
            self.step_to(session, Step::Crediting);
            return Ok(());
        };
        let due = self.due(turn, 0);
        turn.to(path::<Docker>(())).within(due).ask(&create, &Homing { session, run })?;
        self.step_to(session, Step::Housing { made });

        Ok(())
    }

    /// How many of its volumes `session`, starting its run `run`, has had made, while Docker makes the next.
    fn housing(&self, session: u32, run: u32) -> Option<u32> {
        let held = self.held.get(&session)?;
        let Life::Starting { run: starting, step: Step::Housing { made }, .. } = &held.life else {
            return None;
        };

        (*starting == run).then_some(*made)
    }

    /// The start of `session`'s run `run` could not be made from the settings: it is refused as a request Docker could
    /// not be written.
    fn unmade(&mut self, turn: &mut Turn<'_, Self>, session: u32, run: u32) -> Result<(), Fault> {
        let why = Refusal::Docker(DockerError::Unwritable(Unwritable::Volume));

        self.unstarted(turn, session, run, &why)
    }

    /// Moves `session`, starting its run `run`, on to `step`.
    pub(super) fn step_to(&mut self, session: u32, step: Step) {
        if let Some(Life::Starting { step: at, .. }) = self.held.get_mut(&session).map(|held| &mut held.life) {
            *at = step;
        }
    }
}

impl Receives<Answer<VolumeCreated, Homing>> for Sessions {
    /// A volume of the session's is made: the next is, or once each is, the credential is read.
    fn handle(
        &mut self,
        (answered, Homing { session, run }): (Answered<VolumeCreated>, Homing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(made) = self.housing(session, run) else {
            return Ok(());
        };
        match answered {
            Answered::Yes(VolumeCreated::Made) => {}
            Answered::Yes(VolumeCreated::Failed(why)) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(why));
            }
            Answered::No(why) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(DockerError::Unanswered(why)));
            }
        }

        self.house(turn, session, run, made + 1)
    }
}
