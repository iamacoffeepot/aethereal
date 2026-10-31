//! The native host's storage, and the file capability on the headless host (ADR-0032).

use super::*;
use aethereal_actor::{Answered, Tick};
use aethereal_files::{FileRead, ReadFile};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
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

/// A request for the asset at `path`.
fn asset(path: &str) -> ReadFile {
    let Ok(path) = FilePath::parse(path) else {
        panic!("the test's path conforms");
    };

    ReadFile { namespace: Namespace::Assets, path }
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

    let Answered::Yes(FileRead::Read { bytes }) = harness.answer(grass)? else {
        panic!("the file there is read");
    };
    assert_eq!(harness.host().engine().blob(&bytes), Some(&b"grass"[..]));
    let Answered::Yes(FileRead::Missing) = harness.answer(stone)? else {
        panic!("the file not there is missing");
    };

    Ok(())
}
