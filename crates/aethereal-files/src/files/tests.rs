//! The file capability's root, on a desk whose writes a runner of the test's own finishes last asked first, as a
//! host's threads may: writes to one file land in the order they were asked (ADR-0039).

use super::*;
use crate::{Listing, Namespace, Read, Storage, Unlisted, Unread, Writes, Writing};
use aethereal_actor::{JobId, JobType, Service, Services, Spares};
use aethereal_data::{FilePath, KindId};
use aethereal_substrate::harness::{Harness, Steps, Unheard, desk};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Inline, Jobs, Taken, Unbuilt};
use std::future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

desk!(
    /// A desk that keeps nothing: its jobs are run by the runner the test sets on its engine.
    struct Desk(());
);

/// The root begun as a host begins it, its storage served by the runner the test sets.
impl Bootstrap<Desk> for Files {
    const SECTION: &'static str = "files";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Desk, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), FilesConfig)?;

        Ok(())
    }
}

/// A storage that keeps each file in memory, by its path, whatever its namespace. Its lock is its own, as a storage is
/// reached through `&self` from the threads that run jobs; what it holds is read back through the root.
#[derive(Default)]
struct Memory {
    files: Mutex<BTreeMap<FilePath, Vec<u8>>>,
}

impl Memory {
    /// The files it holds, whatever a thread that panicked holding them left.
    fn files(&self) -> MutexGuard<'_, BTreeMap<FilePath, Vec<u8>>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Storage for Memory {
    fn read<'a>(&'a self, _: Namespace, path: &'a FilePath, lent: Spares) -> crate::Reading<'a> {
        let bytes = self.files().get(path).cloned().ok_or(Unread::Missing);

        Box::pin(future::ready(Read { bytes, lent }))
    }

    fn write<'a>(&'a self, _: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        self.files().insert(path.clone(), bytes);

        Box::pin(future::ready(Ok(())))
    }

    fn append<'a>(&'a self, _: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        self.files().entry(path.clone()).or_default().extend(bytes);

        Box::pin(future::ready(Ok(())))
    }

    fn list(&self, _: Namespace) -> Listing<'_> {
        Box::pin(future::ready(Err(Unlisted::NotListed)))
    }
}

/// Runs every job inline as it is handed out, over a [`Memory`], but each write, which it holds until the test lets
/// the one handed out last go: so two writes in flight at once finish in the other order from the one they were asked
/// in, as two of a host's threads may finish them. A runner is handed jobs through `&self`, so the writes it holds are
/// behind a lock of its own: no mail orders it.
struct LastFirst {
    inline: Inline,
    held: Mutex<Vec<(JobId, JobType, Taken)>>,
}

impl LastFirst {
    /// Runs jobs over a new, empty [`Memory`].
    fn new() -> Self {
        let services = Service::<dyn Storage>::of(Arc::new(Memory::default())).added_to(Services::none());

        Self { inline: Inline::new(Arc::new(services)), held: Mutex::default() }
    }

    /// Runs the write it holds that was handed it last.
    fn let_go_last(&self) {
        let last = self.held.lock().unwrap_or_else(PoisonError::into_inner).pop();
        let Some((id, of, job)) = last else {
            panic!("a write is held");
        };
        self.inline.start(id, of, job);
    }
}

impl Jobs for LastFirst {
    fn start(&self, id: JobId, of: JobType, job: Taken) {
        if of.kind() != KindId::of::<Save>() {
            self.inline.start(id, of, job);
            return;
        }
        self.held.lock().unwrap_or_else(PoisonError::into_inner).push((id, of, job));
    }

    fn stop(&self, _: JobId) {}

    fn most(&self) -> usize {
        usize::MAX
    }
}

/// The test's path `path`, which conforms.
fn checked(path: &str) -> FilePath {
    let Ok(path) = FilePath::parse(path) else {
        panic!("the test's path conforms");
    };

    path
}

/// A request to write `bytes` as the whole of the save at `path`, its bytes put in `harness`.
fn save(harness: &mut Harness<Desk>, path: &str, bytes: &[u8]) -> Result<WriteFile, Unheard> {
    let bytes = harness.put(bytes.to_vec())?;

    Ok(WriteFile { namespace: Namespace::Save, path: checked(path), bytes, how: Writes::Whole })
}

#[test]
fn two_writes_to_one_file_land_in_the_order_asked_whatever_order_the_host_finishes_them_in() -> Result<(), Unheard> {
    // Catches a second write to a file run while the first is in flight, which a host's threads may finish first, so
    // the first lands over it and the file holds the older save though both are answered written; and a write held
    // behind another that is never run, or is answered to the other's asker.
    let runner = Arc::new(LastFirst::new());
    let mut harness = Harness::of(Desk::program().with_cycle::<()>().root::<Files>())?;
    Steps::engine(harness.host()).set_jobs(Arc::clone(&runner) as Arc<dyn Jobs>);
    let files = harness.root::<Files>();
    let (first, second) = (save(&mut harness, "slot1", b"first")?, save(&mut harness, "slot1", b"second")?);

    let first = harness.ask(files, &first)?;
    let second = harness.ask(files, &second)?;
    harness.settle();
    runner.let_go_last();
    harness.settle();
    runner.let_go_last();

    let answers = harness.answers((first, second))?;
    assert_eq!(answers, (Answered::Yes(FileWritten::Written), Answered::Yes(FileWritten::Written)));
    let read = harness.ask(files, &ReadFile { namespace: Namespace::Save, path: checked("slot1") })?;
    let Answered::Yes(FileRead::Read { bytes }) = harness.answer(read)? else {
        panic!("the save is read");
    };
    assert_eq!(harness.read(&bytes), Some(&b"second"[..]));

    Ok(())
}
