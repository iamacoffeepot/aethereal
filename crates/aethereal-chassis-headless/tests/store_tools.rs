//! The store's tools on the native host (ADR-0039, ADR-0036): `StoreTools` beside a real store kept in a directory of
//! the test's, the test asking the tools' worker each call as a holder would.

use aethereal_actor::{Answered, ProtocolRef};
use aethereal_chassis_headless::Headless;
use aethereal_data::{
    Budget, JsonValue, Plain, Schema, SchemaNodes, from_bytes, from_json, read_json, to_bytes, to_json,
};
use aethereal_store::{
    CheckInInput, CheckedInOutput, EntryListed, Held, Hold, ListStoredInput, PinInput, PinOutput, Store, StoreHash,
    StoreName, StoreSettings, StoreTools, StoreToolsConfig, StoredListed,
};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_tools::{Call, Called, Tool, Tooling};
use std::path::PathBuf;
use std::{env, fs, process};

/// The most bytes a test's walk takes.
const WALK_BYTES: usize = 1 << 20;

/// A directory for one test, named for it, removed when dropped: the files a test checks in beneath `files/`, and the
/// store kept beneath `store/`.
struct Dir(PathBuf);

impl Dir {
    /// An empty directory for `test`.
    fn new(test: &str) -> Self {
        let root = env::temp_dir().join(format!("aethereal-store-tools-{}-{test}", process::id()));
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

    /// The store's settings, kept here, each file at most `most_file_bytes`.
    fn settings(&self, most_file_bytes: u64) -> StoreSettings {
        StoreSettings { root: self.0.join("store").display().to_string(), most_bytes: 1 << 30, most_file_bytes }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A program holding the store, kept as `settings` say, and its tools, with the tools' worker.
fn store_tools(settings: StoreSettings) -> Result<(Harness<Headless>, ProtocolRef<Tooling>), Unheard> {
    let mut harness = Harness::of(Headless::program().root_with::<Store>(settings))?;
    harness.add_root::<StoreTools>(&(), StoreToolsConfig)?;
    let worker = StoreTools::worker(harness.root::<StoreTools>());

    Ok((harness, worker))
}

/// `value` as bytes.
fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let Ok(bytes) = to_bytes(value) else {
        panic!("the value encodes");
    };

    bytes
}

/// What the tools `worker` answer a call of `tool` with the input `bytes`.
fn called_with(
    harness: &mut Harness<Headless>,
    worker: ProtocolRef<Tooling>,
    tool: &str,
    input: Vec<u8>,
) -> Result<Answered<Called>, Unheard> {
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input })?;

    harness.answer(asked)
}

/// What the tools `worker` answer a call of `tool` with `input`.
fn called<T: Plain>(
    harness: &mut Harness<Headless>,
    worker: ProtocolRef<Tooling>,
    tool: &str,
    input: &T,
) -> Result<Answered<Called>, Unheard> {
    called_with(harness, worker, tool, bytes_of(input))
}

/// The output `called` holds, read as `T`.
fn output<T: Plain>(called: Answered<Called>) -> T {
    let Answered::Yes(Called::Output(bytes)) = called else {
        panic!("the tool answers an output: {called:?}");
    };
    let Ok(output) = from_bytes(&bytes, &mut Budget::new(WALK_BYTES)) else {
        panic!("the output reads as the tool's output kind");
    };

    output
}

/// The reason `called` refuses with.
fn refusal(called: Answered<Called>) -> String {
    let Answered::Yes(Called::Refused(why)) = called else {
        panic!("the tool refuses: {called:?}");
    };

    why
}

/// The input of `check_in` for `path` under `name`.
fn check_in_input(path: String, name: &str) -> CheckInInput {
    CheckInInput { path, name: String::from(name) }
}

/// What `check_in` answers of `path` under `name`, which the test expects to be stored.
fn checked_in(
    harness: &mut Harness<Headless>,
    worker: ProtocolRef<Tooling>,
    path: String,
    name: &str,
) -> Result<CheckedInOutput, Unheard> {
    Ok(output(called(harness, worker, "check_in", &check_in_input(path, name))?))
}

/// What `list_stored` answers.
fn listed(harness: &mut Harness<Headless>, worker: ProtocolRef<Tooling>) -> Result<Vec<EntryListed>, Unheard> {
    let stored: StoredListed = output(called(harness, worker, "list_stored", &ListStoredInput)?);

    Ok(stored.entries)
}

/// The BLAKE3 hash of `bytes`, as the test computes it.
fn hash_of(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

/// The name `text`.
fn name(text: &str) -> StoreName {
    let Ok(name) = StoreName::parse(text) else {
        panic!("the test's name `{text}` conforms");
    };

    name
}

/// The string the JSON object `json` holds at `member`.
fn json_string(json: &str, member: &str) -> String {
    let Ok(tree) = read_json(json, &mut Budget::new(WALK_BYTES)) else {
        panic!("the output is JSON: {json}");
    };
    let JsonValue::Object(span) = tree.root() else {
        panic!("the output is an object: {json}");
    };
    let Some(JsonValue::String(text)) = tree.member(*span, member) else {
        panic!("the output holds the string `{member}`: {json}");
    };

    text.clone()
}

#[test]
fn check_in_answers_the_hash_the_store_holds_as_lowercase_hex_of_its_blake3_in_json() -> Result<(), Unheard> {
    // Catches a hash, size, or freshness lost or swapped converting the store's answer, a hash kind JSON writes as
    // anything but 64 hexadecimal digits (an array of numbers, a newtype's object), so an agent could not read it or
    // write it back, and a check-in the tool answered stored that the store does not hold.
    let dir = Dir::new("hash");
    let bytes = b"a binary, as far as the store can tell".repeat(64);
    let (mut harness, worker) = store_tools(dir.settings(1 << 20))?;

    let answer = checked_in(&mut harness, worker, dir.file("engine", &bytes), "engine")?;

    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    assert_eq!(answer, CheckedInOutput { hash: hash_of(&bytes), bytes: length, fresh: true });
    let listed = listed(&mut harness, worker)?;
    let hashes: Vec<[u8; 32]> = listed.iter().map(|entry| entry.hash).collect();
    assert_eq!(hashes, vec![answer.hash], "the store holds the hash the tool answered");
    let written = to_json(&SchemaNodes::of(&CheckedInOutput::SHAPE), &bytes_of(&answer), &mut Budget::new(WALK_BYTES));
    let Ok(json) = written else {
        panic!("the output writes as JSON");
    };
    assert_eq!(json_string(&json, "hash"), blake3::hash(&bytes).to_hex().as_str());

    Ok(())
}

#[test]
fn list_stored_shows_each_entrys_hash_names_size_pin_and_hold() -> Result<(), Unheard> {
    // Catches an entry's names, size, pin, or hold lost or swapped converting the store's listing to the agent's (the
    // two flags read in the wrong order), and the store's order, last checked in first, not kept.
    let dir = Dir::new("list");
    let (mut harness, worker) = store_tools(dir.settings(1 << 20))?;
    let older = checked_in(&mut harness, worker, dir.file("a", b"pinned build"), "tool")?;
    let newer = checked_in(&mut harness, worker, dir.file("b", b"held"), "game")?;
    let pinned: PinOutput = output(called(&mut harness, worker, "pin", &PinInput { hash: older.hash })?);
    assert_eq!(pinned, PinOutput {});
    let store = harness.root::<Store>();
    let holding = harness.ask(store, &Hold { hash: StoreHash(newer.hash) })?;
    assert_eq!(harness.answer(holding)?, Answered::Yes(Held::Done));

    let entries = listed(&mut harness, worker)?;

    let expected = vec![
        EntryListed { hash: newer.hash, names: vec![name("game")], bytes: 4, pinned: false, held: true },
        EntryListed { hash: older.hash, names: vec![name("tool")], bytes: 12, pinned: true, held: false },
    ];
    assert_eq!(entries, expected);

    Ok(())
}

#[test]
fn pin_written_as_the_hex_the_agent_read_pins_and_unpin_clears_it() -> Result<(), Unheard> {
    // Catches `pin` and `unpin` swapped, as both are answered `Pinned`, a pin that never reaches the store, and a hash
    // the agent writes back in the hexadecimal it read that does not name the entry.
    let dir = Dir::new("pin");
    let bytes = b"keep me";
    let (mut harness, worker) = store_tools(dir.settings(1 << 20))?;
    let stored = checked_in(&mut harness, worker, dir.file("keep", bytes), "keep")?;
    let hex = blake3::hash(bytes).to_hex();
    let typed =
        from_json(&SchemaNodes::of(&PinInput::SHAPE), &format!("{{\"hash\":\"{hex}\"}}"), &mut Budget::new(WALK_BYTES));
    let Ok(input) = typed else {
        panic!("the hexadecimal hash is `pin`'s input");
    };

    let _: PinOutput = output(called_with(&mut harness, worker, "pin", input.clone())?);
    let pinned: Vec<bool> = listed(&mut harness, worker)?.iter().map(|entry| entry.pinned).collect();
    assert_eq!(pinned, vec![true], "the entry {:?} is pinned", stored.hash);

    let _: PinOutput = output(called_with(&mut harness, worker, "unpin", input)?);
    let pinned: Vec<bool> = listed(&mut harness, worker)?.iter().map(|entry| entry.pinned).collect();
    assert_eq!(pinned, vec![false], "the entry's pin is cleared");

    Ok(())
}

#[test]
fn each_case_short_of_done_is_refused_with_its_own_reason() -> Result<(), Unheard> {
    // Catches a refusal the agent cannot act on: a missing path, a directory, a file past the store's limit, and an
    // unknown hash each refused under another case's reason or answered as an output; and a name that does not conform
    // sent to the store anyway, which would move no name but leave the agent reading a decoder's fault.
    let dir = Dir::new("refused");
    let (mut harness, worker) = store_tools(dir.settings(8))?;
    let directory = dir.0.join("files").display().to_string();
    let unknown = PinInput { hash: hash_of(b"never checked in") };

    let missing = called(&mut harness, worker, "check_in", &check_in_input(dir.path("absent"), "tool"))?;
    let not_a_file = called(&mut harness, worker, "check_in", &check_in_input(directory, "tool"))?;
    let too_large = called(&mut harness, worker, "check_in", &check_in_input(dir.file("large", b"9 bytes!!"), "tool"))?;
    let bad_name = called(&mut harness, worker, "check_in", &check_in_input(dir.file("fits", b"8 bytes!"), "../tool"))?;
    let pin_unknown = called(&mut harness, worker, "pin", &unknown)?;
    let unpin_unknown = called(&mut harness, worker, "unpin", &unknown)?;

    assert_eq!(refusal(missing), "nothing is at that path");
    assert_eq!(refusal(not_a_file), "that is not a regular file");
    assert_eq!(refusal(too_large), "the file is larger than the store takes");
    assert_eq!(refusal(bad_name), "a name is 1 to 128 of ASCII letters, digits, `_`, `-`, and `.`");
    assert_eq!(refusal(pin_unknown), "no binary of that hash is stored");
    assert_eq!(refusal(unpin_unknown), "no binary of that hash is stored");
    assert_eq!(listed(&mut harness, worker)?, Vec::new(), "nothing was checked in");

    Ok(())
}
