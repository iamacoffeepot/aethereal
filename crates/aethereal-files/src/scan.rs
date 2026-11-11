//! The job that lists a namespace: made by the file capability alone (ADR-0032, ADR-0054).

use crate::{FilesListed, Namespace, Storage, Unlisted};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Lists the files of `namespace` its listing holds, through the host's storage. It is private to this crate, so no
/// actor but [`Files`](crate::Files) makes one, and what an actor lists goes through the one actor that checks it.
#[derive(Plain)]
#[kind(name = "aether.files.scan")]
pub struct Scan {
    namespace: Namespace,
}

impl Scan {
    pub const fn new(namespace: Namespace) -> Self {
        Self { namespace }
    }
}

/// It waits on the host's storage, as a read does, so it runs where waiting is polled: a native storage reads the
/// directory on threads of its own and wakes it, and a browser's fetch of the manifest is a promise.
impl Job for Scan {
    type Done = FilesListed;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<FilesListed, Fault> {
        let Some(storage) = job.service::<dyn Storage>() else {
            return Ok(FilesListed::Unavailable);
        };
        let listed = storage.list(self.namespace).await;
        let done = match listed {
            Ok(paths) => FilesListed::Listed { paths },
            Err(Unlisted::NotListed) => FilesListed::NotListed,
            Err(Unlisted::Missing) => FilesListed::Missing,
            Err(Unlisted::Unavailable) => FilesListed::Unavailable,
            Err(Unlisted::Failed) => FilesListed::Failed,
        };

        Ok(done)
    }
}
