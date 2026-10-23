//! The job that reads a file: made by the file capability alone (ADR-0032).

use crate::FileRead;
use aethereal_actor::{Fault, Job, Namespace, Runner, Running, Unread};
use aethereal_data::{FilePath, Plain};

/// Reads the whole file at `path` in `namespace` from the host's storage, and puts its bytes as a blob. It is private
/// to this crate, so no actor but [`Files`](crate::Files) makes one, and what an actor reads goes through the one
/// actor that checks it.
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
/// storage reads on threads of its own and wakes it.
impl Job for Load {
    type Done = FileRead;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<FileRead, Fault> {
        let read = job.storage().read(self.namespace, &self.path).await;
        let done = match read {
            Ok(bytes) => FileRead::Read { bytes: job.put(bytes) },
            Err(Unread::Missing) => FileRead::Missing,
            Err(Unread::Unavailable) => FileRead::Unavailable,
            Err(Unread::Failed) => FileRead::Failed,
        };

        Ok(done)
    }
}
