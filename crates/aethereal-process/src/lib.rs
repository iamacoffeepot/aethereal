//! The process capability (ADR-0040): one root, [`Processes`], that starts programs on the host's machine.
//!
//! An actor asks the root to [`Start`] a program, at its typed path,
//! `path::<Processes>(())`, naming the actor it is to tell how the program
//! ended, usually itself, by a reference of the protocol [`Exits`]. Each
//! program running is a [`Process`] beneath the root, which waits for it to
//! exit, tells that actor [`Exited`] with its status and the tail of its
//! stderr, and ends. A process is asked to [`Stop`]: it signals its program's
//! group to end, and kills the group if it has not within the setting
//! `stop_within_millis`. It stops its program too when the actor it tells
//! ends, so no program outlives the actor that wanted it.
//!
//! What starts, waits on, and signals a program is the host's [`Launcher`],
//! reached by jobs only this crate makes. The headless and desktop hosts'
//! crates implement it, bootstrap the root, and serve the launcher into the
//! services their runners hand every job (ADR-0034); a browser bootstraps no
//! processes.

mod jobs;
mod launcher;
mod process;
mod processes;
mod settings;

pub use launcher::{Exiting, Launcher, Signal, Starting, Unstarted, Unwaited};
pub use process::{ExitStatus, Exited, Exits, Process, ProcessConfig, Stop};
pub use processes::{Processes, ProcessesConfig, Start, Started};
pub use settings::ProcessesSettings;
