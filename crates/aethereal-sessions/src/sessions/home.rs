//! A session's home (ADR-0057): a named volume of its own, `aethereal-session-<label>-<n>`, which Docker keeps where it
//! keeps volumes, so the hub never names a path of this machine for it. A volume Docker makes is root's, and the
//! session's container runs as the session's user, as Claude Code refuses to skip its permission checks as root; so
//! before a session's first container, the hub makes its volume and gives it to that user once, with a container of
//! the settings' image run as root that changes the home's owner and ends, and is removed. Archiving a session keeps
//! its volume.

use super::asks::Refusal;
use super::container::{self, Unmade};
use super::credential::{Crediting, ReadCredential};
use super::record::Home;
use super::session::{Life, Step};
use super::{Act, Acting, Acts, Asking, Does, Sessions};
use crate::{
    Acted, ContainerId, Create, CreateVolume, Created, Docker, DockerError, Remove, Start, Unwritable, VolumeCreated,
    Wait, Waited,
};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Turn, path};
use aethereal_data::Plain;

/// What the capability keeps for a request of Docker about a session's home: the session, and the run it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.homing")]
pub struct Homing {
    pub session: u32,
    pub run: u32,
}

impl Sessions {
    /// Asks Docker to make the home of `session`, starting its run `run`, which is at [`Step::Housing`].
    pub(super) fn house<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, run: u32) -> Result<(), Fault> {
        let Ok(create) = self.volume(session) else {
            return self.unmade(turn, session, run);
        };
        let due = self.due(turn, 0);
        turn.to(path::<Docker>(())).within(due).ask(&create, &Homing { session, run })?;

        Ok(())
    }

    /// The request that makes the home of `session`, carrying its labels.
    fn volume(&self, session: u32) -> Result<CreateVolume, Unmade> {
        let name = container::home(&self.settings, session)?;
        let labels = container::labels(&self.settings, session)?;

        Ok(CreateVolume { name, labels })
    }

    /// The home of `session`, starting its run `run`, is its user's: the credential is read next.
    fn owned<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, run: u32) -> Result<(), Fault> {
        if let Some(held) = self.held.get_mut(&session) {
            held.home = Home::Owned;
        }
        turn.run(&ReadCredential, &Crediting { session, run })?;
        self.step_to(session, Step::Crediting);
        self.dirty(session);

        self.flush(turn)
    }

    /// The container that gives `session`'s home to its user, of its run `run`, when the start is at `at`.
    fn owner_at(&self, session: u32, run: u32, at: fn(&Step) -> Option<&ContainerId>) -> Option<ContainerId> {
        let held = self.held.get(&session)?;
        let Life::Starting { run: starting, step, .. } = &held.life else {
            return None;
        };
        let is_run = *starting == run;

        at(step).filter(|_| is_run).cloned()
    }

    /// The start of `session`'s run `run` could not be made from the settings: it is refused as a request Docker could
    /// not be written.
    fn unmade<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, run: u32) -> Result<(), Fault> {
        let why = Refusal::Docker(DockerError::Unwritable(Unwritable::Volume));

        self.unstarted(turn, session, run, &why)
    }

    /// Moves `session`, starting its run `run`, on to `step`.
    pub(super) fn step_to(&mut self, session: u32, step: Step) {
        if let Some(Life::Starting { step: at, .. }) = self.held.get_mut(&session).map(|held| &mut held.life) {
            *at = step;
        }
    }

    /// Asks Docker to remove `owner`, the container that gave `session`'s home to its user, for its run `run`.
    fn disown<S: Does>(
        &self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        owner: ContainerId,
    ) -> Result<(), Fault> {
        let due = self.due(turn, self.settings.grace_secs);
        let acting = Acting { session, run, act: Act::Disown };
        turn.to(path::<Docker>(())).within(due).ask(&Remove { container: owner }, &acting)?;

        Ok(())
    }
}

impl Handles<Answer<VolumeCreated, Homing>> for Sessions {
    type Sends = Acts;

    /// The home's volume is made: the container that gives it to the session's user is.
    fn handle(
        &mut self,
        (answered, Homing { session, run }): (Answered<VolumeCreated>, Homing),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let is_housing = self.is_at(session, run, |step| matches!(step, Step::Housing));
        if !is_housing {
            return Ok(());
        }
        match answered {
            Answered::Yes(VolumeCreated::Made) => {}
            Answered::Yes(VolumeCreated::Failed(why)) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(why));
            }
            Answered::No(why) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(DockerError::Unanswered(why)));
            }
        }
        let spec = match container::owner(&self.settings, session, run) {
            Ok(spec) => spec,
            Err(Unmade::Unlabelled(_) | Unmade::Unnamed) => return self.unmade(turn, session, run),
        };
        let due = self.due(turn, 0);
        turn.to(path::<Docker>(())).within(due).ask(&Create { spec }, &Asking { session, run })?;
        self.step_to(session, Step::Owning);

        self.flush(turn)
    }
}

impl Sessions {
    /// Docker made the container that gives the home of `session`, starting its run `run`, to the session's user, or
    /// did not: it is started.
    pub(super) fn owner_made<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        answered: Answered<Created>,
    ) -> Result<(), Fault> {
        let owner = match answered {
            Answered::Yes(Created::Made(owner)) => owner,
            Answered::Yes(Created::Failed(why)) => return self.unstarted(turn, session, run, &Refusal::Docker(why)),
            Answered::No(why) => {
                return self.unstarted(turn, session, run, &Refusal::Docker(DockerError::Unanswered(why)));
            }
        };
        let due = self.due(turn, 0);
        let acting = Acting { session, run, act: Act::Own };
        turn.to(path::<Docker>(())).within(due).ask(&Start { container: owner.clone() }, &acting)?;
        self.step_to(session, Step::Handing { owner });

        Ok(())
    }

    /// Docker started the container that gives the home of `session`, starting its run `run`, to the session's user,
    /// or did not: one started is waited for.
    pub(super) fn owner_started<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        run: u32,
        acted: Acted,
    ) -> Result<(), Fault> {
        let Some(owner) = self.owner_at(session, run, Step::handing) else {
            return Ok(());
        };
        if let Acted::Failed(why) = acted {
            self.disown(turn, session, run, owner)?;
            return self.unstarted(turn, session, run, &Refusal::Docker(why));
        }
        let due = self.due(turn, 0);
        turn.to(path::<Docker>(())).within(due).ask(&Wait { container: owner.clone() }, &Homing { session, run })?;
        self.step_to(session, Step::Waiting { owner });

        Ok(())
    }

    /// Docker removed the container that gave `session`'s home to its user, or did not: only a failure is logged, as
    /// the start goes on either way.
    pub(super) fn disowned<S: Does>(turn: &mut Turn<'_, Self, S>, session: u32, acted: &Acted) {
        let is_gone = matches!(acted, Acted::Done | Acted::Failed(DockerError::NotFound { .. }));
        if !is_gone {
            turn.log(Level::Warn, format_args!("session {session}'s home's owner was not removed: {acted:?}"));
        }
    }
}

impl Handles<Answer<Waited, Homing>> for Sessions {
    type Sends = Acts;

    /// The container that gives the home to the session's user ended: it is removed, and once it ended well, the home
    /// is the user's and the credential is read.
    fn handle(
        &mut self,
        (answered, Homing { session, run }): (Answered<Waited>, Homing),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let Some(owner) = self.owner_at(session, run, Step::waiting) else {
            return Ok(());
        };
        self.disown(turn, session, run, owner)?;
        let why = match answered {
            Answered::Yes(Waited::Exited { code: 0 }) => return self.owned(turn, session, run),
            Answered::Yes(Waited::Exited { code }) => Refusal::Unowned { code },
            Answered::Yes(Waited::Failed(why)) => Refusal::Docker(why),
            Answered::No(why) => Refusal::Docker(DockerError::Unanswered(why)),
        };

        self.unstarted(turn, session, run, &why)
    }
}

impl Step {
    /// The container that gives the home to the session's user, while Docker starts it.
    const fn handing(&self) -> Option<&ContainerId> {
        match self {
            Self::Handing { owner } => Some(owner),
            _ => None,
        }
    }

    /// The container that gives the home to the session's user, while Docker waits for it to end.
    const fn waiting(&self) -> Option<&ContainerId> {
        match self {
            Self::Waiting { owner } => Some(owner),
            _ => None,
        }
    }
}
