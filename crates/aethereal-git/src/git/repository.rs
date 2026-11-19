//! The repository the root serves (ADR-0057): made as the root begins, by `git init --bare` and then the check that its
//! hook is in place, each a program `Processes` runs, and served once both have exited well.

use super::{Git, Launch, Readiness};
use crate::{GitSettings, programs};
use aethereal_actor::{ActorRef, Answered, Beginning, Fault, Level, Turn, path};
use aethereal_data::{Bound, Plain};
use aethereal_process::{ExitStatus, Exited, Process, Processes, Started};

/// Which of the programs that make the repository runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Step {
    /// `git init --bare`.
    Init,
    /// The check that the hook is in place.
    Check,
}

/// Where the repository stands.
#[derive(Debug, Bound)]
pub(super) enum Repository {
    /// `Processes` is asked to start `step`.
    Starting(Step),
    /// `step` runs as `process`.
    Running { step: Step, process: ActorRef<Process> },
    /// It is made and its hook is in place: it is served.
    Ready,
    /// It could not be made, or its hook is not in place: why is logged, and it is never served.
    Unmade,
}

impl Repository {
    /// Begins making the repository, as the root begins: `git init --bare` started; or, when the hooks are a relative
    /// path, which git would read from the repository and so find no hook, unmade.
    pub(super) fn make(settings: &GitSettings, turn: &mut Beginning<'_, Git>) -> Result<Self, Fault> {
        if !settings.hooks.starts_with('/') {
            turn.log(
                Level::Error,
                format_args!(
                    "the hooks `{}` are not an absolute path, which git would read from the repository, so the \
                     repository is not served",
                    settings.hooks
                ),
            );
            return Ok(Self::Unmade);
        }
        let start = programs::init(settings, turn.me().protocol());
        turn.to(path::<Processes>(())).ask(&start, &Launch::Repository)?;

        Ok(Self::Starting(Step::Init))
    }

    /// Where it stands, as the root says it.
    pub(super) const fn readiness(&self) -> Readiness {
        match self {
            Self::Starting(_) | Self::Running { .. } => Readiness::Making,
            Self::Ready => Readiness::Ready,
            Self::Unmade => Readiness::Unmade,
        }
    }

    /// Whether `process`, the bits of a process's id, is the program that makes it.
    pub(super) fn runs(&self, process: u64) -> bool {
        matches!(self, Self::Running { process: running, .. } if running.id().bits() == process)
    }
}

impl Git {
    /// A program that makes the repository is started, or is not, and then it is never served.
    pub(super) fn repository_started(
        &mut self,
        answered: &Answered<Started>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Repository::Starting(step) = self.repository else {
            return Ok(());
        };
        let &Answered::Yes(Started::Running { process, .. }) = answered else {
            turn.log(Level::Error, format_args!("the repository's {step:?} did not start, so it is not served"));
            self.repository = Repository::Unmade;
            return Ok(());
        };
        self.repository = Repository::Running { step, process };
        let Some(exited) = self.unclaimed.remove(&process.id().bits()) else {
            return Ok(());
        };

        self.repository_exited(exited, turn)
    }

    /// A program that makes the repository has exited: well, and the next is started, or the repository is served
    /// once both have; otherwise it is never served.
    pub(super) fn repository_exited(&mut self, exited: Exited, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Repository::Running { step, .. } = self.repository else {
            return Ok(());
        };
        if exited.status != ExitStatus::Code(0) {
            let Exited { status, stderr } = exited;
            let why = match step {
                Step::Init => format!("could not be made at `{}`", self.settings.repository),
                Step::Check => format!("has no hook git can run at `{}/pre-receive`", self.settings.hooks),
            };
            turn.log(Level::Error, format_args!("the repository {why}, so it is not served: {status:?} {stderr}"));
            self.repository = Repository::Unmade;
            return Ok(());
        }

        match step {
            Step::Init => {
                let start = programs::check(&self.settings, turn.me().protocol());
                turn.to(path::<Processes>(())).ask(&start, &Launch::Repository)?;
                self.repository = Repository::Starting(Step::Check);
            }
            Step::Check => {
                turn.log(Level::Info, format_args!("the repository `{}` is served", self.settings.repository));
                self.repository = Repository::Ready;
            }
        }

        Ok(())
    }
}
