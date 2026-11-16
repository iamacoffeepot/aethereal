//! The job that writes a file: made by the file capability alone (ADR-0039).

use crate::{FileWritten, Namespace, Storage, Unwritten, Writes};
use aethereal_actor::{Bytes, Fault, Job, Runner, Running};
use aethereal_data::{Bound, FilePath};

/// Writes `bytes` to the file at `path` in `namespace` through the host's storage, as `how` says. It is private to this
/// crate, so no actor but [`Files`](crate::Files) makes one, and what an actor writes goes through the one actor that
/// checks it. It is named for what it does, as `Load` is; the namespace it writes is always spelled
/// [`Namespace::Save`].
#[derive(Bound)]
#[kind(name = "aether.files.save")]
pub struct Save {
    namespace: Namespace,
    path: FilePath,
    bytes: Bytes,
    how: Writes,
}

impl Save {
    pub const fn new(namespace: Namespace, path: FilePath, bytes: Bytes, how: Writes) -> Self {
        Self { namespace, path, bytes, how }
    }
}

/// It waits on the host's storage, as a read does, so it runs where waiting is polled: a native storage writes on
/// threads of its own and wakes it, and a browser's writes are promises.
impl Job for Save {
    type Done = FileWritten;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<FileWritten, Fault> {
        let Some(storage) = job.service::<dyn Storage>() else {
            return Ok(FileWritten::Unavailable);
        };
        let Self { namespace, path, bytes, how } = self;
        // The storage writes on threads of its own, or hands the bytes to the page, so it takes a buffer of its own.
        let bytes = job.read(&bytes).to_vec();
        let written = match how {
            Writes::Whole => storage.write(namespace, &path, bytes).await,
            Writes::Append => storage.append(namespace, &path, bytes).await,
        };
        let done = match written {
            Ok(()) => FileWritten::Written,
            Err(Unwritten::ReadOnly) => FileWritten::ReadOnly,
            Err(Unwritten::Unavailable) => FileWritten::Unavailable,
            Err(Unwritten::Failed) => FileWritten::Failed,
        };

        Ok(done)
    }
}
