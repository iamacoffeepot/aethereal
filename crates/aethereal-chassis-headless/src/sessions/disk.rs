//! The sessions' directories on the native host's disk, done on the disk's own thread (ADR-0057).

use aethereal_sessions::{FoundSession, Unkept};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::Path;

/// A session's record.
const RECORD: &str = "record";
/// A record being written, before it is renamed over the old.
const RECORD_WRITING: &str = "record.writing";
/// A session's transcript.
const TRANSCRIPT: &str = "transcript";
/// A session's home, bound into its container.
const HOME: &str = "home";

/// Every session's record and transcript under `root`: each directory named by a number that holds a record. The root
/// is made when it is missing.
pub fn find(root: &Path) -> Result<Vec<FoundSession>, Unkept> {
    fs::create_dir_all(root).map_err(|_| Unkept)?;
    let mut found = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| Unkept)? {
        let entry = entry.map_err(|_| Unkept)?;
        let Some(session) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let directory = entry.path();
        let record = match fs::read(directory.join(RECORD)) {
            Ok(record) => record,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return Err(Unkept),
        };
        let transcript = match fs::read(directory.join(TRANSCRIPT)) {
            Ok(transcript) => transcript,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err(Unkept),
        };
        found.push(FoundSession { session, record, transcript });
    }

    Ok(found)
}

/// Makes the directory of `session` under `root`, and its home, and answers the home's absolute path.
pub fn make(root: &Path, session: u32) -> Result<String, Unkept> {
    let home = root.join(session.to_string()).join(HOME);
    fs::create_dir_all(&home).map_err(|_| Unkept)?;
    let absolute = fs::canonicalize(&home).map_err(|_| Unkept)?;

    absolute.to_str().map(String::from).ok_or(Unkept)
}

/// The credential's file's bytes.
pub fn credential(path: &Path) -> Result<Vec<u8>, Unkept> {
    fs::read(path).map_err(|_| Unkept)
}

/// Appends `frames` to the transcript of `session` under `root`, then writes `record` as its record, beside itself
/// and renamed over the old, so a crash leaves the old record or the new.
pub fn keep(root: &Path, session: u32, frames: &[u8], record: &[u8]) -> Result<(), Unkept> {
    let directory = root.join(session.to_string());
    fs::create_dir_all(&directory).map_err(|_| Unkept)?;
    if !frames.is_empty() {
        let mut transcript =
            OpenOptions::new().create(true).append(true).open(directory.join(TRANSCRIPT)).map_err(|_| Unkept)?;
        transcript.write_all(frames).map_err(|_| Unkept)?;
        transcript.sync_data().map_err(|_| Unkept)?;
    }
    let writing = directory.join(RECORD_WRITING);
    let mut file = File::create(&writing).map_err(|_| Unkept)?;
    file.write_all(record).map_err(|_| Unkept)?;
    file.sync_all().map_err(|_| Unkept)?;
    fs::rename(&writing, directory.join(RECORD)).map_err(|_| Unkept)
}
