//! The credential each session's container is given (ADR-0057): a file of `KEY=VALUE` lines the host reads where the
//! settings name it, as each container is made. It is a secret, so it is never kept in `Files`, whose namespaces any
//! actor may read, and never a setting's value, an argument, a log line, or a record: only the file's path is a
//! setting.

use crate::Variable;
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::{Bound, Plain};
use core::str;

/// The host could not read the credential's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoCredential;

/// The host's read of the credential's file: a service it serves its jobs (ADR-0034). A host that serves none starts
/// no session.
pub trait Credential: Send + Sync {
    /// The credential's file's bytes, read as it is asked, so a credential renewed on disk reaches the next container
    /// made. It is a few lines, read whole on the thread that asks.
    ///
    /// # Errors
    ///
    /// The file could not be read.
    fn read(&self) -> Result<Vec<u8>, NoCredential>;
}

/// Reads the credential, before a session's container is made. Private to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.read_credential")]
pub struct ReadCredential;

/// What the capability keeps for a read of the credential: the session, and the run it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.crediting")]
pub struct Crediting {
    pub session: u32,
    pub run: u32,
}

/// What a [`ReadCredential`] came to.
#[derive(Bound)]
#[kind(name = "aether.sessions.credentialed")]
pub enum Credentialed {
    /// The credential's variables.
    Read(Vec<Variable>),
    /// The host serves no credential, could not read its file, or a line of it is not `KEY=VALUE`.
    Unread,
}

/// It waits on nothing: a host's read is of a few lines and done where the job runs, so it is done in the one poll a
/// job for computing is given.
impl Job for ReadCredential {
    type Done = Credentialed;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Credentialed, Fault> {
        let Some(credential) = job.service::<dyn Credential>() else {
            return Ok(Credentialed::Unread);
        };
        let env = credential.read().map_err(|NoCredential| Unsettable).and_then(|bytes| variables(&bytes));

        Ok(env.map_or(Credentialed::Unread, Credentialed::Read))
    }
}

/// The credential's file could not be read, or does not read as `KEY=VALUE` lines.
struct Unsettable;

/// The variables a credential's file sets: one `KEY=VALUE` a line, blank lines and `#` lines passed over.
///
/// # Errors
///
/// A line is neither, or the file is not text.
fn variables(bytes: &[u8]) -> Result<Vec<Variable>, Unsettable> {
    let text = str::from_utf8(bytes).map_err(|_| Unsettable)?;
    let lines = text.lines().map(str::trim).filter(|line| is_setting(line));

    lines
        .map(|line| {
            let (name, value) = line.split_once('=').ok_or(Unsettable)?;
            Ok(Variable { name: String::from(name.trim()), value: String::from(value.trim()) })
        })
        .collect()
}

/// Whether a line of a credential's file sets a variable: it is not blank, nor a `#` comment.
fn is_setting(line: &str) -> bool {
    let is_blank = line.is_empty();
    let is_comment = line.starts_with('#');

    !is_blank && !is_comment
}
