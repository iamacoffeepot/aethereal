//! The native host's storage, and the file capability on the headless host (ADR-0032).

use super::*;
use crate::Exit;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, On, Path, Tick, Top, Turn, path,
};
use aethereal_data::Bound;
use aethereal_files::{FileRead, ReadFile};
use aethereal_substrate::Host;
use core::time::Duration;
use std::env;
use std::process;

/// A directory of files, named for `test`, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    /// A directory holding `files`, each a path beneath it and its bytes.
    fn new(test: &str, files: &[(&str, &[u8])]) -> Self {
        let root = env::temp_dir().join(format!("aethereal-files-{}-{test}", process::id()));
        for (path, bytes) in files {
            let file = root.join(path);
            let Some(parent) = file.parent() else {
                panic!("a file beneath the directory has a parent");
            };
            let made = fs::create_dir_all(parent).and_then(|()| fs::write(&file, bytes));
            assert!(made.is_ok(), "the test's file `{path}` is written");
        }

        Self(root)
    }

    /// Settings naming it as the assets' directory, read on `readers` threads.
    fn roots(&self, readers: u32) -> FileRoots {
        FileRoots { assets: self.0.display().to_string(), readers }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What `storage` read of the asset at `path`.
fn read(storage: &Directories, path: &str) -> Result<Vec<u8>, Unread> {
    let Ok(path) = FilePath::parse(path) else {
        panic!("the test's path conforms");
    };

    pollster::block_on(storage.read(Namespace::Assets, &path))
}

#[test]
fn a_file_is_read_beneath_its_namespace_s_directory() {
    // Catches a path joined to the wrong directory or not at all, a read whose bytes never wake the job that polls
    // it, and many reads at once on few threads lost or handed to the wrong read.
    let dir = Dir::new("read", &[("tiles/grass.png", b"grass"), ("a.txt", b"a")]);
    let storage = Directories::new(&dir.roots(2));

    assert_eq!(read(&storage, "tiles/grass.png"), Ok(b"grass".to_vec()));
    assert_eq!(read(&storage, "a.txt"), Ok(b"a".to_vec()));
    let paths = vec![FilePath::new("a.txt"); 16];
    let reads: Vec<Reading<'_>> = paths.iter().map(|path| storage.read(Namespace::Assets, path)).collect();
    for reading in reads {
        assert_eq!(pollster::block_on(reading), Ok(b"a".to_vec()));
    }
}

#[test]
fn a_file_that_is_not_there_is_missing() {
    // Catches a missing file, or a directory where a file was asked for, answered as a failure the asker cannot tell
    // from a broken disk.
    let dir = Dir::new("missing", &[("tiles/grass.png", b"grass")]);
    let storage = Directories::new(&dir.roots(1));

    assert_eq!(read(&storage, "tiles/stone.png"), Err(Unread::Missing));
    assert_eq!(read(&storage, "tiles"), Err(Unread::Missing));
}

/// What the reader said became of each read.
static READ: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What a reader keeps for a read: the name it asked for.
#[derive(Plain)]
#[kind(name = "aether.test.files.asked")]
struct Asked {
    path: String,
}

/// What a reader is made from: the paths it asks for.
#[derive(Plain)]
#[kind(name = "aether.test.files.reader_config")]
struct ReaderConfig {
    paths: Vec<String>,
}

/// A root that asks the file capability for each of its paths as it begins, says what became of each, and asks the
/// program to stop once all are answered; or, if fifty ticks go by first, gives up and asks it to stop.
#[derive(Bound)]
#[kind(name = "aether.test.files.reader")]
struct Reader {
    waiting: u64,
    ticks: u64,
}

impl ChildOf for Reader {
    type Parent = Top;
}

impl Actor for Reader {
    type Config = ReaderConfig;
    type Key = ();
    type Handles = (On<Tick>, Answer<FileRead, Asked>);
    type BeginSends = (Asks<Path<Files>, ReadFile, Asked>,);
    type EndSends = ();

    fn begin(config: ReaderConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        let waiting = u64::try_from(config.paths.len()).unwrap_or(u64::MAX);
        for name in config.paths {
            let file = FilePath::parse(name.as_str())?;
            let read = ReadFile { namespace: Namespace::Assets, path: file };
            turn.to(path::<Files>(())).ask(&read, &Asked { path: name })?;
        }

        Ok(Self { waiting, ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Reader {
    const SECTION: &'static str = "reader";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        let paths = vec![String::from("tiles/grass.png"), String::from("tiles/stone.png")];
        cx.begin(&(), ReaderConfig { paths })
    }
}

impl Handles<On<Tick>> for Reader {
    type Sends = ();

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.ticks += 1;
        let is_long = self.ticks == 50;
        if is_long {
            READ.lock().map(|mut read| read.push(String::from("gave up"))).ok();
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<FileRead, Asked>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, asked): (Answered<FileRead>, Asked),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let said = match answered {
            Answered::Yes(FileRead::Read { bytes }) => String::from_utf8_lossy(turn.read(&bytes)).into_owned(),
            Answered::Yes(FileRead::Missing) => String::from("missing"),
            Answered::Yes(FileRead::Unavailable) => String::from("unavailable"),
            Answered::Yes(FileRead::Failed) => String::from("failed"),
            Answered::No(why) => format!("{why:?}"),
        };
        READ.lock().map(|mut read| read.push(format!("{}: {said}", asked.path))).ok();
        self.waiting -= 1;
        let is_done = self.waiting == 0;
        if is_done {
            turn.quit();
        }

        Ok(())
    }
}

#[test]
fn an_actor_reads_an_asset_through_the_file_capability() {
    // Catches a host that serves its runners no storage (every read unavailable) or not the capability's job (every
    // read failed), a read answered to no one or to the wrong request, and a missing file answered as anything but
    // missing. The program lists the capability once, and never its job.
    let dir = Dir::new("capability", &[("tiles/grass.png", b"grass")]);
    let built = Headless::program().with_cycle::<(Tick,)>().root_with::<Files>(dir.roots(2)).root::<Reader>().build();
    let Ok(headless) = built else {
        panic!("the program builds");
    };

    let exit = headless.run(Duration::from_millis(5), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let Ok(read) = READ.lock() else {
        panic!("what was said is not poisoned");
    };
    let mut read = read.clone();
    read.sort();
    assert_eq!(read, ["tiles/grass.png: grass", "tiles/stone.png: missing"]);
}
