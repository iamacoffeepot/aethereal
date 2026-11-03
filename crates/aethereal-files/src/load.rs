//! The job that reads a file: made by the file capability alone (ADR-0032).

use crate::FileRead;
use crate::{Namespace, Read, Storage, Unread};
use aethereal_actor::{Fault, Fills, Job, Runner, Running};
use aethereal_data::{FilePath, Plain};

/// Reads the whole file at `path` in `namespace` from the host's storage, and puts its bytes as a blob. It is private
/// to this crate, so no actor but [`Files`](crate::Files) makes one, and what an actor reads goes through the one
/// actor that checks it. It fills buffers: it is lent the largest buffer `Files` keeps, which the storage reads into
/// when the file fits it (ADR-0042).
#[derive(Plain)]
#[kind(name = "aether.files.load")]
pub struct Load {
    namespace: Namespace,
    path: FilePath,
}

impl Load {
    pub const fn new(namespace: Namespace, path: FilePath) -> Self {
        Self { namespace, path }
    }
}

/// It waits on the host's storage, so it runs where waiting is polled: a fetch in a browser is a promise, and a native
/// storage reads on threads of its own and wakes it. Only the storage learns the file's size, so it is handed the
/// buffer the job was lent to take from, and gives back what it does not take.
impl Job for Load {
    type Done = FileRead;
    const RUNNER: Runner = Runner::Io;
    const FILLS: Fills = Fills::Buffers;

    async fn run(self, job: &mut Running) -> Result<FileRead, Fault> {
        let lent = job.lent();
        let Some(storage) = job.service::<dyn Storage>() else {
            job.give_back(lent);
            return Ok(FileRead::Unavailable);
        };
        let Read { bytes, lent } = storage.read(self.namespace, &self.path, lent).await;
        job.give_back(lent);
        let done = match bytes {
            Ok(bytes) => FileRead::Read { bytes: job.put(bytes) },
            Err(Unread::Missing) => FileRead::Missing,
            Err(Unread::Unavailable) => FileRead::Unavailable,
            Err(Unread::Failed) => FileRead::Failed,
        };

        Ok(done)
    }
}
