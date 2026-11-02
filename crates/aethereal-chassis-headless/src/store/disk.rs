//! What the native stockroom does to the disk, each blocking on its thread (ADR-0039).

use aethereal_store::{Copied, NamesFile, RecordFile, Shelf, StoreHash, Uncopied, Unkept};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

/// The directory beneath the store's that holds each entry's bytes and record.
const ENTRIES: &str = "entries";
/// The file beside it that holds every name.
const NAMES: &str = "names";
/// What follows an entry's hash in its record's name.
const RECORD: &str = ".entry";
/// What follows a file's name as it is written, until it is renamed into place.
const PART: &str = ".part";
/// How many bytes a copy reads at a time.
const CHUNK_BYTES: usize = 1 << 16;

/// What a file in `entries/` is, by its name.
enum Found {
    /// An entry's bytes.
    Bytes(StoreHash),
    /// An entry's record.
    Record(StoreHash),
    /// Neither: a copy or a write a crash cut short.
    Stray,
}

impl Found {
    /// What the file named `name` is.
    fn of(name: &str) -> Self {
        let record = name.strip_suffix(RECORD).and_then(StoreHash::from_hex);
        let bytes = StoreHash::from_hex(name);
        match (record, bytes) {
            (Some(hash), _) => Self::Record(hash),
            (None, Some(hash)) => Self::Bytes(hash),
            (None, None) => Self::Stray,
        }
    }
}

/// The file of the bytes of `hash`.
fn bytes_file(entries: &Path, hash: StoreHash) -> PathBuf {
    entries.join(hash.to_string())
}

/// The file of the record of `hash`.
fn record_file(entries: &Path, hash: StoreHash) -> PathBuf {
    entries.join(format!("{hash}{RECORD}"))
}

/// Copies the file at `path` into the store at `root`, as its copy numbered `number`, hashing as it copies: only a
/// regular file of at most `most_bytes`. It is written beside the entries and renamed to its hash once whole; when an
/// entry of its hash is there already, the copy is removed and the entry kept.
pub fn copy(root: &Path, path: &Path, most_bytes: u64, number: u64) -> Result<Copied, Uncopied> {
    // Asked before the file is opened, so a pipe, which an open would wait on, is never opened.
    let found = fs::metadata(path).map_err(|error| uncopied(error.kind()))?;
    check(&found, most_bytes)?;
    let mut source = File::open(path).map_err(|error| uncopied(error.kind()))?;
    check(&source.metadata().map_err(|_| Uncopied::Failed)?, most_bytes)?;
    let entries = root.join(ENTRIES);
    fs::create_dir_all(&entries).map_err(|_| Uncopied::Failed)?;
    let part = entries.join(format!("{number}{PART}"));
    let streamed = stream(&mut source, &part, most_bytes).and_then(|(hash, bytes)| {
        let entry = bytes_file(&entries, hash);
        let is_kept = entry.exists();
        let placed = if is_kept {
            fs::remove_file(&part)
        } else {
            fs::rename(&part, &entry)
        };
        placed.map(|()| Copied { hash, bytes }).map_err(|_| Uncopied::Failed)
    });
    if streamed.is_err() {
        // A copy that did not finish is let go; one the removal misses is a stray the recount removes.
        let _ = fs::remove_file(&part);
    }

    streamed
}

/// Whether `metadata` is of a file the store copies: a regular file of at most `most_bytes`.
fn check(metadata: &fs::Metadata, most_bytes: u64) -> Result<(), Uncopied> {
    if !metadata.is_file() {
        return Err(Uncopied::NotAFile);
    }
    if metadata.len() > most_bytes {
        return Err(Uncopied::TooLarge);
    }

    Ok(())
}

/// Why a file was not copied, as the disk said.
fn uncopied(kind: ErrorKind) -> Uncopied {
    match kind {
        ErrorKind::NotFound | ErrorKind::NotADirectory => Uncopied::Missing,
        _ => Uncopied::Failed,
    }
}

/// Streams `source` into a new file at `part`, hashing as it goes, then syncs it: its hash and how many bytes it held.
/// A file that grows past `most_bytes` as it is read is too large.
fn stream(source: &mut File, part: &Path, most_bytes: u64) -> Result<(StoreHash, u64), Uncopied> {
    let mut copy = File::create(part).map_err(|_| Uncopied::Failed)?;
    let mut hasher = blake3::Hasher::new();
    let mut chunk = vec![0; CHUNK_BYTES];
    let mut bytes: u64 = 0;
    loop {
        let read = match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(Uncopied::Failed),
        };
        bytes += u64::try_from(read).map_err(|_| Uncopied::Failed)?;
        if bytes > most_bytes {
            return Err(Uncopied::TooLarge);
        }
        hasher.update(&chunk[..read]);
        copy.write_all(&chunk[..read]).map_err(|_| Uncopied::Failed)?;
    }
    copy.sync_all().map_err(|_| Uncopied::Failed)?;

    Ok((StoreHash(*hasher.finalize().as_bytes()), bytes))
}

/// Reads every record and the names file of the store at `root`, removing on the way an entry without its record, a
/// record without its entry, and every stray.
pub fn count(root: &Path) -> Result<Shelf, Unkept> {
    let entries = root.join(ENTRIES);
    fs::create_dir_all(&entries).map_err(|_| Unkept)?;
    let mut bytes = BTreeSet::new();
    let mut records = BTreeSet::new();
    for file in fs::read_dir(&entries).map_err(|_| Unkept)? {
        let file = file.map_err(|_| Unkept)?;
        let name = file.file_name();
        match name.to_str().map_or(Found::Stray, Found::of) {
            Found::Bytes(hash) => {
                bytes.insert(hash);
            }
            Found::Record(hash) => {
                records.insert(hash);
            }
            // One the removal misses is tried again at the next recount.
            Found::Stray => {
                let _ = fs::remove_file(file.path());
            }
        }
    }
    for hash in bytes.difference(&records) {
        let _ = fs::remove_file(bytes_file(&entries, *hash));
    }
    for hash in records.difference(&bytes) {
        let _ = fs::remove_file(record_file(&entries, *hash));
    }
    let records = bytes
        .intersection(&records)
        .map(|hash| fs::read(record_file(&entries, *hash)).map(|bytes| RecordFile { hash: *hash, bytes }))
        .collect::<Result<Vec<RecordFile>, io::Error>>()
        .map_err(|_| Unkept)?;
    let names = match fs::read(root.join(NAMES)) {
        Ok(bytes) => NamesFile::Read(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => NamesFile::Absent,
        Err(_) => return Err(Unkept),
    };

    Ok(Shelf { records, names })
}

/// Writes each of `records` beside its entry, then `names` as the names file, of the store at `root`, each whole.
pub fn write(root: &Path, records: &[RecordFile], names: &[u8]) -> Result<(), Unkept> {
    let entries = root.join(ENTRIES);
    fs::create_dir_all(&entries).map_err(|_| Unkept)?;
    for RecordFile { hash, bytes } in records {
        write_whole(&record_file(&entries, *hash), bytes).map_err(|_| Unkept)?;
    }

    write_whole(&root.join(NAMES), names).map_err(|_| Unkept)
}

/// Writes `bytes` to a file beside `file`, syncs it, and renames it over `file`, so a crash leaves the old or the new.
fn write_whole(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut part = OsString::from(file);
    part.push(PART);
    let mut written = File::create(&part)?;
    written.write_all(bytes)?;
    written.sync_all()?;

    fs::rename(&part, file)
}

/// Removes the bytes and the record of each of `hashes` from the store at `root`. One already gone is not missed;
/// any other that is not removed is told once all have been tried.
pub fn prune(root: &Path, hashes: &[StoreHash]) -> Result<(), Unkept> {
    let entries = root.join(ENTRIES);
    let mut kept = false;
    for hash in hashes {
        for file in [bytes_file(&entries, *hash), record_file(&entries, *hash)] {
            let removed = fs::remove_file(file);
            let is_left = removed.is_err_and(|error| error.kind() != ErrorKind::NotFound);
            kept |= is_left;
        }
    }
    if kept {
        return Err(Unkept);
    }

    Ok(())
}
