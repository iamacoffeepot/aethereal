//! The native host's stockroom: the store kept in a directory, its disk worked on a thread of its own (ADR-0039).

mod disk;

use crate::Headless;
use crate::errands::Errands;
use aethereal_actor::Service;
use aethereal_store::{
    Copying, DropName, Keeping, Launching, RecordFile, Recounting, Stockroom, Store, StoreHash, StoreSettings,
    Uncopied, Unkept,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The native host's stockroom (ADR-0039, ADR-0040): the store's directory, `entries/` holding each entry's bytes as
/// `<hash>` and its record as `<hash>.entry`, beside the `names` file, `running/`, which holds the launch copies, each
/// marked executable where the host marks files so, and `drops/`, which holds each session's drop box, `drops/<n>`, the
/// one directory of this machine its container is given (ADR-0057). A session's container runs as the user the hub
/// does, so it writes its box, and the hub reads what it put there. What it is asked is done on a thread of its own, in the order
/// asked, so the disk never blocks the thread that polls it, and woken once done. One thread is enough: the store asks
/// one thing at a time. Dropping it stops its thread once it has finished what it was doing.
pub struct StoreDirectory {
    /// The store's directory.
    root: PathBuf,
    errands: Errands,
    /// The number the next copy's file is written under, until it is renamed to its hash. An atomic, as the store's
    /// jobs ask from the runners' threads, whichever polls them; a copy from a run before that crashed is removed by the
    /// recount, so the numbers need not outlive the process.
    next_copy: AtomicU64,
}

impl StoreDirectory {
    /// The stockroom keeping the store in the directory `settings` names, made as it is first used.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(settings: &StoreSettings) -> Self {
        Self {
            root: PathBuf::from(&settings.root),
            errands: Errands::new("aethereal-store"),
            next_copy: AtomicU64::new(0),
        }
    }
}

impl Stockroom for StoreDirectory {
    fn copy(&self, path: String, most_bytes: u64) -> Copying<'_> {
        let root = self.root.clone();
        let number = self.next_copy.fetch_add(1, Ordering::Relaxed);
        let copying = self.errands.ask(move || disk::copy(&root, &PathBuf::from(path), most_bytes, number));

        Box::pin(async move { copying.await.unwrap_or(Err(Uncopied::Failed)) })
    }

    fn copy_dropped(&self, session: u32, file: DropName, most_bytes: u64) -> Copying<'_> {
        let root = self.root.clone();
        let number = self.next_copy.fetch_add(1, Ordering::Relaxed);
        let copying = self.errands.ask(move || {
            let Some(file) = file.as_plain() else {
                return Err(Uncopied::Missing);
            };
            disk::copy_dropped(&root, session, file, most_bytes, number)
        });

        Box::pin(async move { copying.await.unwrap_or(Err(Uncopied::Failed)) })
    }

    fn drop_box(&self, session: u32) -> Launching<'_> {
        let root = self.root.clone();
        let making = self.errands.ask(move || disk::drop_box(&root, session));

        Box::pin(async move { making.await.unwrap_or(Err(Unkept)) })
    }

    fn count(&self) -> Recounting<'_> {
        let root = self.root.clone();
        let counting = self.errands.ask(move || disk::count(&root));

        Box::pin(async move { counting.await.unwrap_or(Err(Unkept)) })
    }

    fn write(&self, records: Vec<RecordFile>, names: Vec<u8>) -> Keeping<'_> {
        let root = self.root.clone();
        let writing = self.errands.ask(move || disk::write(&root, &records, &names));

        Box::pin(async move { writing.await.unwrap_or(Err(Unkept)) })
    }

    fn prune(&self, hashes: Vec<StoreHash>) -> Keeping<'_> {
        let root = self.root.clone();
        let pruning = self.errands.ask(move || disk::prune(&root, &hashes));

        Box::pin(async move { pruning.await.unwrap_or(Err(Unkept)) })
    }

    fn launch(&self, hash: StoreHash, copy: String) -> Launching<'_> {
        let root = self.root.clone();
        let launching = self.errands.ask(move || disk::launch(&root, hash, &copy));

        Box::pin(async move { launching.await.unwrap_or(Err(Unkept)) })
    }

    fn unlaunch(&self, copies: Vec<String>) -> Keeping<'_> {
        let root = self.root.clone();
        let unlaunching = self.errands.ask(move || disk::unlaunch(&root, &copies));

        Box::pin(async move { unlaunching.await.unwrap_or(Err(Unkept)) })
    }
}

/// The store on the native host: its stockroom the directory its settings name, served to the host for its runners
/// (ADR-0039).
impl Bootstrap<Headless> for Store {
    const SECTION: &'static str = "store";
    type Settings = StoreSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: StoreSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Stockroom>::of(Arc::new(StoreDirectory::new(&settings))));
        cx.begin(&(), settings.config())?;

        Ok(())
    }
}
