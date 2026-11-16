//! Where the sessions capability keeps its sessions: the host's disk, a service the host serves its jobs (ADR-0034,
//! ADR-0057), and the jobs only this crate makes of it.

use super::record::Record;
use crate::Variable;
use aethereal_actor::{Bytes, Fault, Job, Runner, Running};
use aethereal_data::{Bound, Budget, Plain, from_bytes};
use core::str;
use std::future::Future;
use std::pin::Pin;

/// The most bytes a session's record holds as it is read back.
const MOST_RECORD_BYTES: usize = 64 * 1024;

/// The host could not do what was asked of its disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unkept;

/// One session's directory, as the disk found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSession {
    /// Its number.
    pub session: u32,
    /// Its record, as the capability last wrote it.
    pub record: Vec<u8>,
    /// Its transcript's file, every frame appended to it.
    pub transcript: Vec<u8>,
}

/// A read of every session the disk keeps, as it does it.
pub type Finding<'a> = Pin<Box<dyn Future<Output = Result<Vec<FoundSession>, Unkept>> + 'a>>;
/// The making of a session's directories, as the disk does it: the absolute path of its home once made.
pub type Making<'a> = Pin<Box<dyn Future<Output = Result<String, Unkept>> + 'a>>;
/// A read of the credential's file, as the disk does it.
pub type Fetching<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, Unkept>> + 'a>>;
/// A write, as the disk does it.
pub type Storing<'a> = Pin<Box<dyn Future<Output = Result<(), Unkept>> + 'a>>;

/// The host's disk, where each session's record, transcript, and home are kept, a directory each under the root the
/// settings name, and where the credential's file is read (ADR-0057). A host serves it into the services its runners
/// hand every job (ADR-0034); a host that serves none keeps no session.
///
/// Nothing it does blocks the thread that polls it. The capability asks one write at a time, so its writes land in the
/// order made.
pub trait SessionsDisk: Send + Sync {
    /// Reads every session's record and transcript.
    fn find(&self) -> Finding<'_>;

    /// Makes the directories of the session numbered `session`, its home among them, and answers its home's absolute
    /// path, which its container is bound.
    fn make(&self, session: u32) -> Making<'_>;

    /// Reads the credential's file. Its bytes are never logged nor written anywhere.
    fn credential(&self) -> Fetching<'_>;

    /// Appends `frames` to the session's transcript, and then writes `record`, when there is one, as its record
    /// whole: beside itself, then renamed over the old one.
    fn keep(&self, session: u32, frames: Vec<u8>, record: Vec<u8>) -> Storing<'_>;
}

/// Reads every session the disk keeps, as the capability begins. Private to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.find")]
pub struct Find;

/// What the capability keeps for its read of the disk: nothing, as it reads once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.finding")]
pub struct Loading;

/// What a [`Find`] read.
#[derive(Debug, Bound)]
#[kind(name = "aether.sessions.found")]
pub enum Found {
    /// Each session whose record reads, with its transcript.
    Read(Vec<Kept>),
    /// The host serves no disk, or could not read it.
    Failed,
}

/// One session read back: its record, and its transcript's file, in a blob (ADR-0042), as a transcript may hold more
/// than a piece of mail.
#[derive(Debug, Bound)]
#[kind(name = "aether.sessions.kept")]
pub struct Kept {
    pub session: u32,
    pub record: Record,
    pub transcript: Bytes,
}

impl Job for Find {
    type Done = Found;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Found, Fault> {
        let Some(disk) = job.service::<dyn SessionsDisk>() else {
            return Ok(Found::Failed);
        };
        let Ok(found) = disk.find().await else {
            return Ok(Found::Failed);
        };
        let mut kept = Vec::new();
        for FoundSession { session, record, transcript } in found {
            let Ok(record) = from_bytes::<Record>(&record, &mut Budget::new(MOST_RECORD_BYTES)) else {
                continue;
            };
            kept.push(Kept { session, record, transcript: job.put(transcript) });
        }

        Ok(Found::Read(kept))
    }
}

/// Makes a session's directories and reads the credential, before its container is made. Private to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.prepare")]
pub struct Prepare {
    pub session: u32,
}

/// What the capability keeps for a preparation: the session, and the run it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.preparing")]
pub struct Preparing {
    pub session: u32,
    pub run: u32,
}

/// What a [`Prepare`] came to.
#[derive(Bound)]
#[kind(name = "aether.sessions.prepared")]
pub enum Prepared {
    /// The session's home, by its absolute path, and the credential's variables.
    Ready { home: String, env: Vec<Variable> },
    /// The host serves no disk, or could not make the directories.
    Unmade,
    /// The credential's file could not be read, or a line of it is not `KEY=VALUE`.
    NoCredential,
}

impl Job for Prepare {
    type Done = Prepared;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Prepared, Fault> {
        let Some(disk) = job.service::<dyn SessionsDisk>() else {
            return Ok(Prepared::Unmade);
        };
        let Ok(home) = disk.make(self.session).await else {
            return Ok(Prepared::Unmade);
        };
        let env = disk.credential().await.map_err(|_| Unsettable).and_then(|bytes| variables(&bytes));

        Ok(env.map_or(Prepared::NoCredential, |env| Prepared::Ready { home, env }))
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

/// Appends to sessions' transcripts and writes their records, each session's in turn. Private to this crate.
#[derive(Bound)]
#[kind(name = "aether.sessions.keep")]
pub struct Keep {
    pub writes: Vec<Write>,
}

/// One session's write: frames to append to its transcript, and its record, rewritten.
#[derive(Bound)]
#[kind(name = "aether.sessions.write")]
pub struct Write {
    pub session: u32,
    pub frames: Bytes,
    pub record: Bytes,
}

/// What the capability keeps for a write: nothing, as it runs one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.keeping")]
pub struct Keeping;

/// What a [`Keep`] came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.kept_all")]
pub enum Wrote {
    /// Every write was done.
    Done,
    /// The host serves no disk, or could not do every write.
    Failed,
}

impl Job for Keep {
    type Done = Wrote;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Wrote, Fault> {
        let Some(disk) = job.service::<dyn SessionsDisk>() else {
            return Ok(Wrote::Failed);
        };
        let mut wrote = Wrote::Done;
        for Write { session, frames, record } in self.writes {
            let frames = job.read(&frames).to_vec();
            let record = job.read(&record).to_vec();
            let kept = disk.keep(session, frames, record).await;
            if kept.is_err() {
                wrote = Wrote::Failed;
            }
        }

        Ok(wrote)
    }
}
