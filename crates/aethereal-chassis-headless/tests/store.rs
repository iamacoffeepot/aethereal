//! The store on the native host (ADR-0039, ADR-0036): a real program, the store's root asked through the harness, and
//! its stockroom a real directory, worked on its own thread.

use aethereal_actor::Held as Debt;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path, ToSender, Top, Turn, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_data::{Bound, Plain};
use aethereal_store::{
    CheckIn, CheckedIn, Entry, Held, Hold, ListStored, Pin, Pinned, Store, StoreHash, StoreName, StoreSettings,
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
    let CheckedIn::Stored { hash, fresh, .. } = check_in(harness, path, under)? else {
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
    assert_eq!(checked_in, CheckedIn::Stored { hash, bytes: length, fresh: true });
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

/// An actor that holds entries for the test, as the next ADR's spawner will, and ends when told.
#[derive(Bound)]
#[kind(name = "aether.test.holder")]
struct Holder {
    /// The test's asks, in the order it asked the store.
    waiting: Vec<Debt<Held>>,
}

impl ChildOf for Holder {
    type Parent = Top;
}

impl Carries<HoldFor> for Holder {
    type Answers = (Held,);
}

impl Actor for Holder {
    type Config = HolderConfig;
    type Key = ();
    type Handles = (HoldFor, Leave, Answer<Held, Holding>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: HolderConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { waiting: Vec::new() })
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
