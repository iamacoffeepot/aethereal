//! What each session's container is given beside its settings (ADR-0057), readied by one job as each container is
//! made: the credential, a file of `KEY=VALUE` lines the host reads where the settings name it; the bearer of the
//! session's route to the hub's tools, drawn from the host's randomness; and the session's drop box, which the store's
//! stockroom makes. The credential and the bearer are secrets, so neither is ever kept in `Files`, whose namespaces any
//! actor may read, nor a setting's value, an argument, a log line, or a record: only the credential's path is a
//! setting, and each crosses the engine only as this job's answer and the create Docker is asked, as the container's
//! environment, and the bearer as the `Admit` that tells `Mcp` of it, each `Debug` written without it.

use crate::Variable;
use aethereal_actor::{Fault, Job, Randomness, Runner, Running};
use aethereal_data::{Bound, Plain};
use aethereal_mcp::Bearer;
use aethereal_store::Stockroom;
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

/// Readies what the container of the session numbered `session` is given, before it is made: the credential read, a
/// bearer drawn, and the drop box made. Private to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.provision")]
pub struct Provision {
    pub session: u32,
}

/// What the capability keeps for a read of the credential: the session, and the run it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.crediting")]
pub struct Crediting {
    pub session: u32,
    pub run: u32,
}

/// What a [`Provision`] came to.
#[derive(Bound)]
#[kind(name = "aether.sessions.provisioned")]
pub enum Provisioned {
    /// The credential's variables, the bearer of the session's route, and the absolute path of its drop box.
    Ready { env: Vec<Variable>, bearer: Bearer, drop_box: String },
    /// The host serves no credential, could not read its file, or a line of it is not `KEY=VALUE`.
    Unread,
    /// The host serves no randomness, or it filled nothing.
    Undrawn,
    /// The host serves no stockroom, or it could not make the drop box.
    Unboxed,
}

/// It waits on the stockroom, which makes the drop box on a thread of its own; the credential's read is of a few lines
/// and the randomness answers at once, each done where the job runs.
impl Job for Provision {
    type Done = Provisioned;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Provisioned, Fault> {
        let Some(credential) = job.service::<dyn Credential>() else {
            return Ok(Provisioned::Unread);
        };
        let Ok(env) = credential.read().map_err(|NoCredential| Unsettable).and_then(|bytes| variables(&bytes)) else {
            return Ok(Provisioned::Unread);
        };
        let Some(randomness) = job.service::<dyn Randomness>() else {
            return Ok(Provisioned::Undrawn);
        };
        let mut bytes = [0; 16];
        if randomness.fill(&mut bytes).is_err() {
            return Ok(Provisioned::Undrawn);
        }
        let Some(stockroom) = job.service::<dyn Stockroom>() else {
            return Ok(Provisioned::Unboxed);
        };
        let Ok(drop_box) = stockroom.drop_box(self.session).await else {
            return Ok(Provisioned::Unboxed);
        };

        Ok(Provisioned::Ready { env, bearer: Bearer(bytes), drop_box })
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
