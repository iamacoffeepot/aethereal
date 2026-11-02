//! The store on the native host (ADR-0039, ADR-0040, ADR-0036): a real program, the store's root asked through the
//! harness, and its stockroom a real directory, worked on its own thread.

use aethereal_actor::Held as Debt;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path, ToSender, Top, Turn, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_data::{Bound, Budget, DecodeError, Plain, Schema, SchemaNodes, from_bytes, from_json};
use aethereal_store::{
    Binary, CheckIn, CheckedIn, Entry, Held, Hold, Launch, Launched, ListStored, Pin, Pinned, Store, StoreHash,
    StoreName, StoreSettings, StoredBinary,
};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use std::path::PathBuf;
use std::{env, fs, process};

/// A directory for one test, named for it, removed when dropped: the files a test checks in beneath `files/`, and the
/// store kept beneath `store/`.
struct Dir(PathBuf);

impl Dir {
    /// An empty directory for `test`.
    fn new(test: &str) -> Self {
        let root = env::temp_dir().join(format!("aethereal-store-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&root);
        let made = fs::create_dir_all(root.join("files"));
        assert!(made.is_ok(), "the test's directory is made");

        Self(root)
    }

    /// Writes `bytes` as the file `name` beneath `files/`, and answers with its path.
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let file = self.0.join("files").join(name);
        assert!(fs::write(&file, bytes).is_ok(), "the test's file `{name}` is written");

        file.display().to_string()
    }

    /// The path of `name` beneath `files/`, which the test may not have written.
    fn path(&self, name: &str) -> String {
        self.0.join("files").join(name).display().to_string()
    }

    /// The store's `entries/` directory.
    fn entries(&self) -> PathBuf {
        self.0.join("store").join("entries")
    }

    /// The store's `running/` directory, of launch copies.
    fn running(&self) -> PathBuf {
        self.0.join("store").join("running")
    }

    /// The names of the launch copies in `running/`, in order.
    fn launch_copies(&self) -> Vec<String> {
        let Ok(read) = fs::read_dir(self.running()) else {
            return Vec::new();
        };
        let mut names: Vec<String> =
            read.filter_map(Result::ok).map(|file| file.file_name().to_string_lossy().into_owned()).collect();
        names.sort();

        names
    }

    /// The names of what lies in `entries/`, in order.
    fn listed(&self) -> Vec<String> {
        let Ok(read) = fs::read_dir(self.entries()) else {
            return Vec::new();
        };
        let mut names: Vec<String> =
            read.filter_map(Result::ok).map(|file| file.file_name().to_string_lossy().into_owned()).collect();
        names.sort();

        names
    }

    /// The store's settings, kept here, within `most_bytes`, each file at most `most_file_bytes`.
    fn settings(&self, most_bytes: u64, most_file_bytes: u64) -> StoreSettings {
        StoreSettings { root: self.0.join("store").display().to_string(), most_bytes, most_file_bytes }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The BLAKE3 hash of `bytes`, as the test computes it.
fn hash_of(bytes: &[u8]) -> StoreHash {
    StoreHash(*blake3::hash(bytes).as_bytes())
}

/// The name `text`.
fn name(text: &str) -> StoreName {
    let Ok(name) = StoreName::parse(text) else {
        panic!("the test's name `{text}` conforms");
    };

    name
}

/// A program holding the store, kept as `settings` say, built and started.
fn store(settings: StoreSettings) -> Result<Harness<Headless>, Unheard> {
    Harness::of(Headless::program().root_with::<Store>(settings))
}

/// What checking `path` in under `under` comes to.
fn check_in(harness: &mut Harness<Headless>, path: &str, under: &str) -> Result<CheckedIn, Unheard> {
    let store = harness.root::<Store>();
    let asked = harness.ask(store, &CheckIn { path: String::from(path), name: name(under) })?;
    let Answered::Yes(checked_in) = harness.answer(asked)? else {
        panic!("the store answers a check-in");
    };

    Ok(checked_in)
}

/// What checking `path` in under `under` comes to, which the test expects to be stored: its hash and freshness.
fn stored(harness: &mut Harness<Headless>, path: &str, under: &str) -> Result<(StoreHash, bool), Unheard> {
    let CheckedIn::Stored(StoredBinary { hash, fresh, .. }) = check_in(harness, path, under)? else {
        panic!("`{path}` is checked in");
    };

    Ok((hash, fresh))
}

/// Every entry the store lists.
fn entries(harness: &mut Harness<Headless>) -> Result<Vec<Entry>, Unheard> {
    let store = harness.root::<Store>();
    let asked = harness.ask(store, &ListStored)?;
    let Answered::Yes(stored) = harness.answer(asked)? else {
        panic!("the store answers a listing");
    };

    Ok(stored.entries)
}

/// The hashes the store lists, in its order.
fn hashes(harness: &mut Harness<Headless>) -> Result<Vec<StoreHash>, Unheard> {
    Ok(entries(harness)?.into_iter().map(|entry| entry.hash).collect())
}

/// What the store answers the harness's launch of `binary`. The harness lives as long as the test, so its holds and
/// launch copies do too.
fn launch(harness: &mut Harness<Headless>, binary: Binary) -> Result<Launched, Unheard> {
    let store = harness.root::<Store>();
    let asked = harness.ask(store, &Launch { binary })?;
    let Answered::Yes(launched) = harness.answer(asked)? else {
        panic!("the store answers a launch");
    };

    Ok(launched)
}

/// The launch copy the harness's launch of `binary` is answered with, which the test expects to be made, and the hash
/// of the entry it copies.
fn launched(harness: &mut Harness<Headless>, binary: &Binary) -> Result<(PathBuf, StoreHash), Unheard> {
    let Launched::Ready { program, hash } = launch(harness, binary.clone())? else {
        panic!("{binary:?} is launched");
    };

    Ok((PathBuf::from(program), hash))
}

/// Waits until every write the store has asked of its stockroom is done. The store runs its stockroom's jobs one at a
/// time, in order, so once a copy asked after them is answered, they are done: the copy of a path with nothing at it,
/// which writes nothing itself.
fn written(harness: &mut Harness<Headless>, dir: &Dir) -> Result<(), Unheard> {
    let missing = check_in(harness, &dir.path("nothing-here"), "barrier")?;
    assert_eq!(missing, CheckedIn::Missing, "the barrier's path holds nothing");

    Ok(())
}

#[test]
fn a_check_in_keeps_the_bytes_under_their_blake3_hash() -> Result<(), Unheard> {
    // Catches a hash of anything but the file's bytes (a hash of the path, or of a chunk), an entry's file named other
    // than by the hash's digits, a copy cut short or never renamed into place, and an entry listed without its name or
    // size.
    let dir = Dir::new("hash");
    let bytes = b"a binary, as far as the store can tell".repeat(4096);
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;

    let checked_in = check_in(&mut harness, &dir.file("engine", &bytes), "engine")?;

    let hash = hash_of(&bytes);
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    assert_eq!(checked_in, CheckedIn::Stored(StoredBinary { hash, bytes: length, fresh: true }));
    assert_eq!(fs::read(dir.entries().join(hash.to_string())).ok(), Some(bytes));
    let entry = Entry { hash, names: vec![name("engine")], bytes: length, pinned: false, held: false };
    assert_eq!(entries(&mut harness)?, vec![entry]);

    Ok(())
}

#[test]
fn the_same_bytes_checked_in_twice_are_one_entry() -> Result<(), Unheard> {
    // Catches bytes already held stored as a second entry or answered fresh, and a copy that is let go but left in the
    // store's directory.
    let dir = Dir::new("twice");
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;

    let (first, first_fresh) = stored(&mut harness, &dir.file("a", b"same bytes"), "one")?;
    let (second, second_fresh) = stored(&mut harness, &dir.file("b", b"same bytes"), "two")?;

    assert_eq!(second, first);
    assert!(first_fresh, "the first copy is of new bytes");
    assert!(!second_fresh, "the second copy found its bytes held");
    let listed = entries(&mut harness)?;
    assert_eq!(listed.len(), 1, "one entry: {listed:?}");
    assert_eq!(listed[0].names, vec![name("one"), name("two")]);
    written(&mut harness, &dir)?;
    assert_eq!(dir.listed(), vec![first.to_string(), format!("{first}.entry")], "one entry's files, and no copy left");

    Ok(())
}

#[test]
fn a_name_moves_to_the_newer_check_in_and_the_older_build_stays() -> Result<(), Unheard> {
    // Catches a name bound to its first hash, a name kept on both builds, an older build dropped as its name moves
    // (before any budget asks it), and a listing in any order but the last checked in first.
    let dir = Dir::new("moves");
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;

    let (older, _) = stored(&mut harness, &dir.file("old", b"build one"), "game")?;
    let (newer, _) = stored(&mut harness, &dir.file("new", b"build two"), "game")?;

    let listed = entries(&mut harness)?;
    let named: Vec<(StoreHash, Vec<StoreName>)> = listed.into_iter().map(|entry| (entry.hash, entry.names)).collect();
    assert_eq!(named, vec![(newer, vec![name("game")]), (older, Vec::new())]);

    Ok(())
}

#[test]
fn what_is_not_a_small_regular_file_is_refused() -> Result<(), Unheard> {
    // Catches a directory read as a file or answered missing, a missing path answered as a failure the caller cannot
    // tell from a broken disk, a size limit not checked or off by one, and a refused check-in that moves its name or
    // leaves an entry.
    let dir = Dir::new("refused");
    let mut harness = store(dir.settings(1 << 30, 8))?;
    let (kept, _) = stored(&mut harness, &dir.file("fits", b"8 bytes!"), "tool")?;

    let directory = dir.0.join("files").display().to_string();
    assert_eq!(check_in(&mut harness, &directory, "tool")?, CheckedIn::NotAFile);
    assert_eq!(check_in(&mut harness, &dir.path("absent"), "tool")?, CheckedIn::Missing);
    assert_eq!(check_in(&mut harness, &dir.file("large", b"9 bytes!!"), "tool")?, CheckedIn::TooLarge);

    let listed = entries(&mut harness)?;
    let named: Vec<(StoreHash, Vec<StoreName>)> = listed.into_iter().map(|entry| (entry.hash, entry.names)).collect();
    assert_eq!(named, vec![(kept, vec![name("tool")])]);

    Ok(())
}

#[test]
fn pruning_drops_the_least_recently_used_entry_nothing_keeps() -> Result<(), Unheard> {
    // Catches pruning by age alone, which would drop the named, pinned, and held entries checked in before the
    // unnamed ones; a pin or a hold that does not keep; pruning the most recently used first; pruning more than the
    // budget asks; and an entry dropped from the index whose bytes stay on disk.
    let dir = Dir::new("prune");
    let mut harness = store(dir.settings(24, 1 << 10))?;
    let store_ref = harness.root::<Store>();
    let (named, _) = stored(&mut harness, &dir.file("n", b"name"), "keep")?;
    let (pinned, _) = stored(&mut harness, &dir.file("p", b"pin!"), "scratch")?;
    let (held, _) = stored(&mut harness, &dir.file("h", b"held"), "scratch")?;
    let (oldest, _) = stored(&mut harness, &dir.file("u1", b"old1"), "scratch")?;
    let (older, _) = stored(&mut harness, &dir.file("u2", b"old2"), "scratch")?;
    let (last, _) = stored(&mut harness, &dir.file("s", b"last"), "scratch")?;
    let pinning = harness.ask(store_ref, &Pin { hash: pinned })?;
    let holding = harness.ask(store_ref, &Hold { hash: held })?;
    assert_eq!(harness.answers((pinning, holding))?, (Answered::Yes(Pinned::Done), Answered::Yes(Held::Done)));

    let (newest, _) = stored(&mut harness, &dir.file("t", b"over"), "other")?;

    let mut kept = hashes(&mut harness)?;
    kept.sort();
    let mut expected = vec![named, pinned, held, older, last, newest];
    expected.sort();
    assert_eq!(kept, expected, "only the oldest unnamed entry nothing keeps is pruned");
    written(&mut harness, &dir)?;
    let listed = dir.listed();
    assert!(!listed.contains(&oldest.to_string()), "its bytes are removed: {listed:?}");
    assert!(!listed.contains(&format!("{oldest}.entry")), "its record is removed: {listed:?}");

    Ok(())
}

/// What the test's holder is made from: nothing.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.holder_config")]
struct HolderConfig;

/// Hold the entry `hash` in the store: asked of the test's holder, and answered with what the store answered.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.hold_for")]
struct HoldFor {
    hash: StoreHash,
}

/// End: told to the test's holder.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.leave")]
struct Leave;

/// What the holder keeps for a hold it asked: nothing, as it answers its asks in order.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.holding")]
struct Holding;

/// Launch `binary` from the store: asked of the test's holder, and answered with what the store answered.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.launch_for")]
struct LaunchFor {
    binary: Binary,
}

/// What the holder keeps for a launch it asked: nothing, as it answers its asks in order.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.launching")]
struct Launching;

/// An actor that holds and launches entries for the test, as ADR-0040's spawner does, and ends when told.
#[derive(Bound)]
#[kind(name = "aether.test.holder")]
struct Holder {
    /// The test's holds, in the order it asked the store.
    waiting: Vec<Debt<Held>>,
    /// The test's launches, in the order it asked the store.
    launching: Vec<Debt<Launched>>,
}

impl ChildOf for Holder {
    type Parent = Top;
}

impl Carries<HoldFor> for Holder {
    type Answers = (Held,);
}

impl Carries<LaunchFor> for Holder {
    type Answers = (Launched,);
}

impl Actor for Holder {
    type Config = HolderConfig;
    type Key = ();
    type Handles = (HoldFor, LaunchFor, Leave, Answer<Held, Holding>, Answer<Launched, Launching>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: HolderConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { waiting: Vec::new(), launching: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<HoldFor> for Holder {
    type Sends = (Asks<Path<Store>, Hold, Holding>, ToSender<Held>);

    fn handle(&mut self, HoldFor { hash }: HoldFor, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        turn.to(path::<Store>(())).ask(&Hold { hash }, &Holding)?;
        self.waiting.push(debt);

        Ok(())
    }
}

impl Handles<LaunchFor> for Holder {
    type Sends = (Asks<Path<Store>, Launch, Launching>, ToSender<Launched>);

    fn handle(&mut self, LaunchFor { binary }: LaunchFor, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let debt = turn.hold()?;
        turn.to(path::<Store>(())).ask(&Launch { binary }, &Launching)?;
        self.launching.push(debt);

        Ok(())
    }
}

impl Handles<Answer<Launched, Launching>> for Holder {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Launching): (Answered<Launched>, Launching),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(launched) = answered else {
            panic!("the store answers a launch");
        };
        let debt = self.launching.remove(0);
        turn.answer(debt, &launched)?;

        Ok(())
    }
}

impl Handles<Leave> for Holder {
    type Sends = ();

    fn handle(&mut self, _: Leave, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

impl Handles<Answer<Held, Holding>> for Holder {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Holding): (Answered<Held>, Holding),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(held) = answered else {
            panic!("the store answers a hold");
        };
        let debt = self.waiting.remove(0);
        turn.answer(debt, &held)?;

        Ok(())
    }
}

#[test]
fn a_hold_keeps_an_entry_until_its_holder_ends() -> Result<(), Unheard> {
    // Catches a hold that does not keep its entry from pruning, a check-in that cannot fit pruning what it may anyway
    // or leaving its copy on disk, a hold that outlives its holder (the store never watching it, or dropping the
    // holds of the wrong actor), and a hold answered for an entry the store does not have.
    let dir = Dir::new("hold");
    let mut harness = store(dir.settings(8, 1 << 10))?;
    let (gone, _) = stored(&mut harness, &dir.file("a", b"aaaa"), "x")?;
    let (named, _) = stored(&mut harness, &dir.file("b", b"bbbb"), "x")?;
    harness.add_root::<Holder>(&(), HolderConfig)?;
    let holder = harness.root::<Holder>();
    let holding = harness.ask(holder, &HoldFor { hash: gone })?;
    let nothing = harness.ask(holder, &HoldFor { hash: hash_of(b"never checked in") })?;
    assert_eq!(harness.answers((holding, nothing))?, (Answered::Yes(Held::Done), Answered::Yes(Held::NotStored)));
    let held = entries(&mut harness)?.into_iter().find(|entry| entry.hash == gone).map(|entry| entry.held);
    assert_eq!(held, Some(true), "the entry is listed held");

    let blocked = check_in(&mut harness, &dir.file("c", b"cccc"), "y")?;

    assert_eq!(blocked, CheckedIn::Full, "nothing may be pruned while the entry is held");
    assert_eq!(hashes(&mut harness)?, vec![named, gone]);
    written(&mut harness, &dir)?;
    let listed = dir.listed();
    assert!(!listed.contains(&hash_of(b"cccc").to_string()), "the copy that did not fit is removed: {listed:?}");

    harness.tell(holder, &Leave)?;
    harness.settle();
    let (fits, _) = stored(&mut harness, &dir.path("c"), "y")?;

    assert_eq!(hashes(&mut harness)?, vec![fits, named], "once its holder ended, the entry was pruned");

    Ok(())
}

#[test]
fn names_and_pins_are_back_after_a_restart() -> Result<(), Unheard> {
    // Catches an index kept only in memory, records or names never written or not read back, entry files whose names
    // the recount does not read as hashes, a listing order lost across the restart, and a hold written to disk.
    let dir = Dir::new("restart");
    let settings = dir.settings(1 << 30, 1 << 30);
    let (older, newer) = {
        let mut harness = store(settings.clone())?;
        let store_ref = harness.root::<Store>();
        let (older, _) = stored(&mut harness, &dir.file("old", b"release one"), "release")?;
        let (newer, _) = stored(&mut harness, &dir.file("new", b"release two"), "release")?;
        let pinning = harness.ask(store_ref, &Pin { hash: older })?;
        let holding = harness.ask(store_ref, &Hold { hash: newer })?;
        assert_eq!(harness.answers((pinning, holding))?, (Answered::Yes(Pinned::Done), Answered::Yes(Held::Done)));
        written(&mut harness, &dir)?;

        (older, newer)
    };

    let mut harness = store(settings)?;

    let release = vec![name("release")];
    let back = vec![
        Entry { hash: newer, names: release, bytes: 11, pinned: false, held: false },
        Entry { hash: older, names: Vec::new(), bytes: 11, pinned: true, held: false },
    ];
    assert_eq!(entries(&mut harness)?, back);

    Ok(())
}

#[test]
fn what_a_crash_cut_short_is_removed_as_the_store_begins() -> Result<(), Unheard> {
    // Catches a recount that indexes an entry without its record or a record without its entry, that leaves a copy
    // or a write never renamed into place, or that takes a record that does not read for a real one.
    let dir = Dir::new("crash");
    let entries_dir = dir.entries();
    assert!(fs::create_dir_all(&entries_dir).is_ok(), "the store's directory is made");
    let bare = hash_of(b"bytes without a record");
    let unrecorded = hash_of(b"a record without bytes");
    let garbled = hash_of(b"a record that does not read");
    let strays = [
        bare.to_string(),
        format!("{unrecorded}.entry"),
        garbled.to_string(),
        format!("{garbled}.entry"),
        String::from("7.part"),
        format!("{bare}.entry.part"),
    ];
    for stray in &strays {
        assert!(fs::write(entries_dir.join(stray), b"?").is_ok(), "the stray `{stray}` is written");
    }

    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;

    assert_eq!(entries(&mut harness)?, Vec::new());
    assert_eq!(dir.listed(), Vec::<String>::new());

    Ok(())
}

/// The mode bits of `file` that say who may read, write, and run it.
#[cfg(unix)]
fn mode_of(file: &PathBuf) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = fs::metadata(file) else {
        panic!("the file {} is there", file.display());
    };

    metadata.permissions().mode() & 0o777
}

#[test]
fn a_launch_by_name_copies_the_entry_to_an_executable_file_named_for_it() -> Result<(), Unheard> {
    // Catches a launch copy of anything but the named entry's bytes (another entry, or a copy cut short), one not
    // marked executable, one named for the hash where the process would show it, a path that is relative or not
    // beneath the store's `running/`, an answer of the wrong hash, and the entry itself left executable or moved.
    let dir = Dir::new("launch-name");
    let bytes = b"an engine, as far as the store can tell".repeat(4096);
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;
    let (other, _) = stored(&mut harness, &dir.file("other", b"another build"), "other")?;
    let (hash, _) = stored(&mut harness, &dir.file("engine", &bytes), "engine")?;

    let (program, launched_hash) = launched(&mut harness, &Binary::Named(name("engine")))?;

    assert_eq!(launched_hash, hash);
    assert_ne!(launched_hash, other);
    assert!(program.is_absolute(), "the launch copy's path is absolute: {}", program.display());
    assert_eq!(program, dir.running().join("engine-0"));
    assert_eq!(fs::read(&program).ok(), Some(bytes.clone()));
    assert_eq!(fs::read(dir.entries().join(hash.to_string())).ok(), Some(bytes), "the entry stays");
    #[cfg(unix)]
    {
        assert_eq!(mode_of(&program), 0o755, "the launch copy is executable");
        assert_eq!(mode_of(&dir.entries().join(hash.to_string())) & 0o111, 0, "the entry is not");
    }

    Ok(())
}

#[test]
fn a_launch_by_hash_is_named_for_a_name_pointing_at_it_or_for_its_digits() -> Result<(), Unheard> {
    // Catches a launch by hash that finds nothing or the wrong entry, a launch copy of an entry asked by hash named for
    // a name that has moved off it or for a name other than the first in order, and a name or hash the store does not
    // hold answered as anything but not stored.
    let dir = Dir::new("launch-hash");
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;
    let (named, _) = stored(&mut harness, &dir.file("a", b"twice named"), "zeta")?;
    let _ = stored(&mut harness, &dir.file("b", b"twice named"), "alpha")?;
    let (moved_off, _) = stored(&mut harness, &dir.file("c", b"first game"), "game")?;
    let _ = stored(&mut harness, &dir.file("d", b"second game"), "game")?;

    let (by_name, named_hash) = launched(&mut harness, &Binary::Hash(named))?;
    let (by_digits, digits_hash) = launched(&mut harness, &Binary::Hash(moved_off))?;

    assert_eq!((named_hash, digits_hash), (named, moved_off));
    assert_eq!(by_name, dir.running().join("alpha-0"));
    let digits: String = moved_off.to_string().chars().take(12).collect();
    assert_eq!(by_digits, dir.running().join(format!("{digits}-1")));
    assert_eq!(fs::read(&by_digits).ok(), Some(b"first game".to_vec()));
    let unknown_name = launch(&mut harness, Binary::Named(name("never-checked-in")))?;
    let unknown_hash = launch(&mut harness, Binary::Hash(hash_of(b"never checked in")))?;
    assert_eq!((unknown_name, unknown_hash), (Launched::NotStored, Launched::NotStored));

    Ok(())
}

#[test]
fn a_launch_holds_its_build_until_the_asker_ends_and_then_its_copy_goes() -> Result<(), Unheard> {
    // Catches a launch that does not hold its entry, so a check-in prunes the build an engine runs; a store that never
    // watches the asker, so the hold and the launch copy outlive it; and the launch copies of every asker removed when
    // one ends, or of none.
    let dir = Dir::new("launch-hold");
    let mut harness = store(dir.settings(8, 1 << 10))?;
    let (launched_hash, _) = stored(&mut harness, &dir.file("a", b"aaaa"), "x")?;
    let (named, _) = stored(&mut harness, &dir.file("b", b"bbbb"), "x")?;
    harness.add_root::<Holder>(&(), HolderConfig)?;
    let holder = harness.root::<Holder>();
    let asked = harness.ask(holder, &LaunchFor { binary: Binary::Hash(launched_hash) })?;
    let Answered::Yes(Launched::Ready { program, .. }) = harness.answer(asked)? else {
        panic!("the holder's launch is made");
    };
    let (kept_program, _) = launched(&mut harness, &Binary::Named(name("x")))?;
    let held = entries(&mut harness)?.into_iter().find(|entry| entry.hash == launched_hash).map(|entry| entry.held);
    assert_eq!(held, Some(true), "the launched entry is listed held");

    let blocked = check_in(&mut harness, &dir.file("c", b"cccc"), "y")?;

    assert_eq!(blocked, CheckedIn::Full, "nothing may be pruned while the launched entry is held");
    assert_eq!(hashes(&mut harness)?, vec![named, launched_hash]);

    harness.tell(holder, &Leave)?;
    harness.settle();
    written(&mut harness, &dir)?;

    assert!(!PathBuf::from(&program).exists(), "the ended asker's launch copy is removed");
    assert!(kept_program.exists(), "the live asker's launch copy stays");
    let (fits, _) = stored(&mut harness, &dir.path("c"), "y")?;
    assert_eq!(hashes(&mut harness)?, vec![fits, named], "once its asker ended, the launched entry was pruned");

    Ok(())
}

#[test]
fn each_launch_has_its_own_copy_which_a_rebuild_leaves_alone() -> Result<(), Unheard> {
    // Catches a count that does not move, so a second launch writes over the first's copy as it runs, and a launch
    // copy that follows its name to a newer build checked in under it.
    let dir = Dir::new("launch-twice");
    let mut harness = store(dir.settings(1 << 30, 1 << 30))?;
    let _ = stored(&mut harness, &dir.file("old", b"build one"), "game")?;
    let (first, _) = launched(&mut harness, &Binary::Named(name("game")))?;
    let _ = stored(&mut harness, &dir.file("new", b"build two"), "game")?;

    let (second, _) = launched(&mut harness, &Binary::Named(name("game")))?;

    assert_ne!(first, second);
    assert_eq!(fs::read(&first).ok(), Some(b"build one".to_vec()), "the first launch copy is its own build");
    assert_eq!(fs::read(&second).ok(), Some(b"build two".to_vec()));
    assert_eq!(dir.launch_copies(), vec![String::from("game-0"), String::from("game-1")]);

    Ok(())
}

#[test]
fn launch_copies_left_behind_are_removed_as_the_store_begins() -> Result<(), Unheard> {
    // Catches a store that leaves the launch copies of a run that ended without its askers ending, as a crash does,
    // and a file a crash left half written in `running/`, so they pile up across restarts.
    let dir = Dir::new("launch-restart");
    let settings = dir.settings(1 << 30, 1 << 30);
    {
        let mut harness = store(settings.clone())?;
        let _ = stored(&mut harness, &dir.file("engine", b"engine"), "engine")?;
        let _ = launched(&mut harness, &Binary::Named(name("engine")))?;
    }
    assert!(fs::write(dir.running().join("engine-7.part"), b"?").is_ok(), "the half-written copy is written");
    assert_eq!(dir.launch_copies(), vec![String::from("engine-0"), String::from("engine-7.part")]);

    let mut harness = store(settings)?;
    let _ = entries(&mut harness)?;

    assert_eq!(dir.launch_copies(), Vec::<String>::new());

    Ok(())
}

#[test]
fn a_check_in_under_a_name_that_spells_a_hash_is_refused_where_it_is_read() {
    // Catches a name rule that lets a hash's 64 lowercase hexadecimal digits through, so the agent's one string for a
    // binary could name two entries, and a refusal that does not tell the caller the rule it broke.
    let dir = Dir::new("hash-name");
    let path = dir.file("engine", b"engine").replace('\\', "\\\\");
    let digits = hash_of(b"engine");
    let json = format!("{{\"path\":\"{path}\",\"name\":\"{digits}\"}}");
    let Ok(bytes) = from_json(&SchemaNodes::of(&CheckIn::SHAPE), &json, &mut Budget::new(1 << 16)) else {
        panic!("a name of any text is a string, so the JSON reads as a check-in's shape");
    };

    let read = from_bytes::<CheckIn>(&bytes, &mut Budget::new(1 << 16));

    let rule = "a name is 1 to 128 of ASCII letters, digits, `_`, `-`, and `.`, and not the 64 lowercase hexadecimal \
                digits that spell a hash";
    assert_eq!(read, Err(DecodeError::Refused { name: "StoreName", rule }));
}
