//! The native host's storage, and the file capability on the headless host (ADR-0032, ADR-0039).

use super::*;
use aethereal_actor::{Answered, Tick};
use aethereal_files::{FileRead, FileWritten, ReadFile, WriteFile};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use std::env;
use std::process;

/// A directory of files, named for `test`, removed when dropped: the assets' directory and the saves' beside it.
struct Dir(PathBuf);

impl Dir {
    /// A directory whose assets are `files`, each a path beneath the assets' directory and its bytes, and with no
    /// saves.
    fn new(test: &str, files: &[(&str, &[u8])]) -> Self {
        let root = env::temp_dir().join(format!("aethereal-files-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, bytes) in files {
            let file = root.join("assets").join(path);
            let Some(parent) = file.parent() else {
                panic!("a file beneath the directory has a parent");
            };
            let made = fs::create_dir_all(parent).and_then(|()| fs::write(&file, bytes));
            assert!(made.is_ok(), "the test's file `{path}` is written");
        }

        Self(root)
    }

    /// Settings naming its assets' and saves' directories, read and written on `readers` threads.
    fn roots(&self, readers: u32) -> FileRoots {
        let assets = self.0.join("assets").display().to_string();
        let save = self.0.join("save").display().to_string();

        FileRoots { assets, save, readers }
    }

    /// Where the file at `path` in `namespace` is on the disk.
    fn file(&self, namespace: Namespace, path: &str) -> PathBuf {
        let directory = match namespace {
            Namespace::Assets => "assets",
            Namespace::Save => "save",
        };

        self.0.join(directory).join(path)
    }

    /// The names in the directory holding the save at `path`.
    fn beside(&self, path: &str) -> Vec<String> {
        let Some(directory) = self.file(Namespace::Save, path).parent().map(Path::to_path_buf) else {
            panic!("a save has a directory");
        };
        let Ok(entries) = fs::read_dir(directory) else {
            panic!("the save's directory is there");
        };
        let mut names: Vec<String> =
            entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        names.sort();

        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The test's path `path`, which conforms.
fn checked(path: &str) -> FilePath {
    let Ok(path) = FilePath::parse(path) else {
        panic!("the test's path conforms");
    };

    path
}

/// What `storage` read of the asset at `path`.
fn read(storage: &Directories, path: &str) -> Result<Vec<u8>, Unread> {
    read_in(storage, Namespace::Assets, path)
}

/// What `storage` read of the file at `path` in `namespace`.
fn read_in(storage: &Directories, namespace: Namespace, path: &str) -> Result<Vec<u8>, Unread> {
    pollster::block_on(storage.read(namespace, &checked(path)))
}

/// What became of `storage` writing `bytes` as the file at `path` in `namespace`.
fn write(storage: &Directories, namespace: Namespace, path: &str, bytes: &[u8]) -> Result<(), Unwritten> {
    pollster::block_on(storage.write(namespace, &checked(path), bytes.to_vec()))
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

#[test]
fn a_save_is_written_whole_beneath_directories_it_makes_and_read_back() {
    // Catches a write to the wrong directory or a read of `Save` from the assets', directories a path names left
    // unmade (every first save in a slot failing), and a write whose bytes never wake the job that polls it.
    let dir = Dir::new("save", &[]);
    let storage = Directories::new(&dir.roots(2));

    assert_eq!(write(&storage, Namespace::Save, "slots/one/game.sav", b"saved"), Ok(()));

    assert_eq!(read_in(&storage, Namespace::Save, "slots/one/game.sav"), Ok(b"saved".to_vec()));
    assert_eq!(fs::read(dir.file(Namespace::Save, "slots/one/game.sav")).ok(), Some(b"saved".to_vec()));
}

#[test]
fn a_write_replaces_the_file_whole_and_leaves_no_sibling() {
    // Catches a write into the file where it is, which keeps the tail of a longer file it replaces and lets a reader
    // see half of it, and a sibling left beside the file once it was renamed or failed.
    let dir = Dir::new("replace", &[]);
    let storage = Directories::new(&dir.roots(1));

    assert_eq!(write(&storage, Namespace::Save, "game.sav", b"a long first save"), Ok(()));
    assert_eq!(write(&storage, Namespace::Save, "game.sav", b"short"), Ok(()));

    assert_eq!(read_in(&storage, Namespace::Save, "game.sav"), Ok(b"short".to_vec()));
    assert_eq!(dir.beside("game.sav"), vec![String::from("game.sav")]);
}

#[test]
fn writes_of_one_file_at_once_each_land_whole() {
    // Catches two writes sharing one sibling, which write over each other's bytes or rename the other's away, and so
    // fail or leave a file of neither.
    let dir = Dir::new("together", &[]);
    let storage = Directories::new(&dir.roots(4));
    let path = checked("game.sav");
    let saves: Vec<Vec<u8>> = (0..16u8).map(|number| vec![number; 4096]).collect();

    let writes: Vec<Writing<'_>> =
        saves.iter().map(|bytes| storage.write(Namespace::Save, &path, bytes.clone())).collect();
    for writing in writes {
        assert_eq!(pollster::block_on(writing), Ok(()));
    }

    let Ok(last) = read_in(&storage, Namespace::Save, "game.sav") else {
        panic!("the file is there");
    };
    assert!(saves.contains(&last), "the file is one save whole");
    assert_eq!(dir.beside("game.sav"), vec![String::from("game.sav")]);
}

#[test]
fn a_write_beneath_a_file_or_over_a_directory_fails() {
    // Catches a write the disk refused answered as done, and a failed write that leaves its sibling behind.
    let dir = Dir::new("refused", &[]);
    let storage = Directories::new(&dir.roots(1));
    assert_eq!(write(&storage, Namespace::Save, "game.sav", b"saved"), Ok(()));
    assert_eq!(write(&storage, Namespace::Save, "slots/one.sav", b"saved"), Ok(()));

    assert_eq!(write(&storage, Namespace::Save, "game.sav/inner.sav", b"inner"), Err(Unwritten::Failed));
    assert_eq!(write(&storage, Namespace::Save, "slots", b"over"), Err(Unwritten::Failed));

    assert_eq!(read_in(&storage, Namespace::Save, "game.sav"), Ok(b"saved".to_vec()));
    assert_eq!(dir.beside("slots/one.sav"), vec![String::from("one.sav")]);
}

#[test]
fn the_storage_writes_no_asset() {
    // Catches a storage that writes the files the program ships with when asked directly, past the capability's
    // check.
    let dir = Dir::new("assets", &[("a.txt", b"a")]);
    let storage = Directories::new(&dir.roots(1));

    assert_eq!(write(&storage, Namespace::Assets, "a.txt", b"b"), Err(Unwritten::ReadOnly));
    assert_eq!(write(&storage, Namespace::Assets, "new.txt", b"b"), Err(Unwritten::ReadOnly));

    assert_eq!(read(&storage, "a.txt"), Ok(b"a".to_vec()));
    assert!(!dir.file(Namespace::Assets, "new.txt").exists(), "no asset is made");
}

/// A request for the asset at `path`.
fn asset(path: &str) -> ReadFile {
    ReadFile { namespace: Namespace::Assets, path: checked(path) }
}

/// A request to write `bytes` as the file at `path` in `namespace`.
fn write_file(namespace: Namespace, path: &str, bytes: &[u8]) -> WriteFile {
    WriteFile { namespace, path: checked(path), bytes: bytes.to_vec() }
}

#[test]
fn an_actor_reads_an_asset_through_the_file_capability() -> Result<(), Unheard> {
    // Catches a host that serves its runners no storage (every read unavailable) or not the capability's job (every
    // read failed), a read answered to no one or to the wrong request, and a missing file answered as anything but
    // missing. The program lists the capability once, and never its job.
    let dir = Dir::new("capability", &[("tiles/grass.png", b"grass")]);
    let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root_with::<Files>(dir.roots(2)))?;
    let files = harness.root::<Files>();

    let grass = harness.ask(files, &asset("tiles/grass.png"))?;
    let stone = harness.ask(files, &asset("tiles/stone.png"))?;

    let (grass, stone) = harness.answers((grass, stone))?;

    let Answered::Yes(FileRead::Read { bytes }) = grass else {
        panic!("the file there is read");
    };
    assert_eq!(harness.host().engine().blob(&bytes), Some(&b"grass"[..]));
    let Answered::Yes(FileRead::Missing) = stone else {
        panic!("the file not there is missing");
    };

    Ok(())
}

#[test]
fn an_actor_writes_a_save_and_reads_it_back_through_the_file_capability() -> Result<(), Unheard> {
    // Catches a write the capability does not hold or answers to the wrong request, a host that serves no storage
    // for writes or not the capability's job, and a read of `Save` that reads the assets.
    let dir = Dir::new("capability-save", &[("game.sav", b"an asset")]);
    let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root_with::<Files>(dir.roots(2)))?;
    let files = harness.root::<Files>();

    let first = harness.ask(files, &write_file(Namespace::Save, "slots/game.sav", b"first"))?;
    let other = harness.ask(files, &write_file(Namespace::Save, "other.sav", b"other"))?;
    let (first, other) = harness.answers((first, other))?;
    assert_eq!((first, other), (Answered::Yes(FileWritten::Written), Answered::Yes(FileWritten::Written)));

    let read = harness.ask(files, &ReadFile { namespace: Namespace::Save, path: checked("slots/game.sav") })?;
    let Answered::Yes(FileRead::Read { bytes }) = harness.answer(read)? else {
        panic!("the save written is read");
    };
    assert_eq!(harness.host().engine().blob(&bytes), Some(&b"first"[..]));

    Ok(())
}

#[test]
fn a_write_to_assets_is_read_only_in_the_turn_that_asks() -> Result<(), Unheard> {
    // Catches a write to `Assets` handed to a job, which `Files` would take a second turn to answer and which leaves
    // the storage's own refusal the only guard, and one that writes the asset.
    let dir = Dir::new("capability-assets", &[("a.txt", b"a")]);
    let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root_with::<Files>(dir.roots(2)))?;
    let files = harness.root::<Files>();
    harness.host().engine_mut().record_turns();

    let written = harness.ask(files, &write_file(Namespace::Assets, "a.txt", b"b"))?;
    assert_eq!(harness.answer(written)?, Answered::Yes(FileWritten::ReadOnly));

    let turns_of_files =
        harness.host().engine_mut().take_turned().iter().filter(|turn| turn.actor == files.seated()).count();
    assert_eq!(turns_of_files, 1, "the asking turn alone");
    assert_eq!(fs::read(dir.file(Namespace::Assets, "a.txt")).ok(), Some(b"a".to_vec()));

    Ok(())
}
