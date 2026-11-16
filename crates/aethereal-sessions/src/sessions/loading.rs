//! How the capability begins (ADR-0057): it reads its sessions through `Files`, the index and then each session's
//! record and transcript, follows Docker's events for its label, and only then lists the containers carrying it, so
//! nothing that happens to one falls between the two. Each container a record says may still run is re-attached when
//! it does, and each it has no record of is reported, never removed.

use super::asked::Waiting;
use super::record::{Record, Standing};
use super::saves::{Index, Loading, record, transcript};
use super::session::{Life, Session};
use super::transcript::{Ended, Entry, Note, Transcript};
use super::{Acts, Phase, Reconciling, Recorded, Sessions, Unfound};
use crate::{ContainerId, Docker, Events, List, Listed, Listing, Phase as DockerPhase};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Turn, path};
use aethereal_data::{Budget, Plain, from_bytes};
use aethereal_files::{FileRead, Files, Namespace, ReadFile};
use core::mem;
use std::collections::BTreeMap;

/// The most bytes the index holds as it is read back: past a million sessions' numbers.
const MOST_INDEX_BYTES: usize = 4 << 20;

/// The most bytes a session's record holds as it is read back.
const MOST_RECORD_BYTES: usize = 64 * 1024;

/// What the capability has read of its sessions as it begins: each record that reads, each transcript, and how many
/// reads are not answered yet.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Reading {
    records: BTreeMap<u32, Record>,
    transcripts: BTreeMap<u32, Transcript>,
    out: u32,
}

impl Handles<Answer<FileRead, Loading>> for Sessions {
    type Sends = Acts;

    /// A file of the sessions read: the index asks for each session's record and transcript, and once every one is
    /// answered, the sessions are held as stopped until the containers carrying the hub's label are listed.
    fn handle(
        &mut self,
        (answered, loading): (Answered<FileRead>, Loading),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let read = match answered {
            Answered::Yes(FileRead::Read { bytes }) => Read::Bytes(turn.read(&bytes).to_vec()),
            Answered::Yes(FileRead::Missing) => Read::Missing,
            Answered::Yes(FileRead::Unavailable) => return unread(turn, loading, "Files has nowhere to read it from"),
            Answered::Yes(FileRead::Failed) => return unread(turn, loading, "Files could not read it"),
            Answered::No(why) => return unread(turn, loading, &format!("Files answered nothing: {why:?}")),
        };
        match (mem::replace(&mut self.phase, Phase::Indexing { waiting: Vec::new() }), loading) {
            (Phase::Indexing { waiting }, Loading::Index) => self.indexed(turn, &read, waiting),
            (Phase::Reading { mut reading, waiting }, Loading::Record { session }) => {
                if let Read::Bytes(bytes) = read {
                    // A record is written whole, so one that does not read is not this hub's: its session is left out.
                    if let Ok(record) = from_bytes::<Record>(&bytes, &mut Budget::new(MOST_RECORD_BYTES)) {
                        reading.records.insert(session, record);
                    }
                }
                self.answered(turn, reading, waiting)
            }
            (Phase::Reading { mut reading, waiting }, Loading::Transcript { session }) => {
                if let Read::Bytes(bytes) = read {
                    reading.transcripts.insert(session, Transcript::read(&bytes));
                }
                self.answered(turn, reading, waiting)
            }
            (phase, _) => {
                self.phase = phase;
                Ok(())
            }
        }
    }
}

/// The capability's failure to begin, as the sessions' file `loading` reads was not read, for `why`: it cannot tell
/// what sessions it has.
fn unread(turn: &mut Turn<'_, Sessions, Acts>, loading: Loading, why: &str) -> Result<(), Fault> {
    turn.log(Level::Error, format_args!("the sessions' file of {loading:?} was not read: {why}"));

    Err(Fault::new(&Unfound::Files)?)
}

/// What a read of one of the sessions' files found.
enum Read {
    Bytes(Vec<u8>),
    /// No file is there: no index is no session, and no transcript an empty one.
    Missing,
}

impl Sessions {
    /// The index read as `read`: each session it lists has its record and transcript asked for; with none, the
    /// sessions are found.
    fn indexed(&mut self, turn: &mut Turn<'_, Self, Acts>, read: &Read, waiting: Vec<Waiting>) -> Result<(), Fault> {
        let sessions = match read {
            Read::Bytes(bytes) => from_bytes::<Index>(bytes, &mut Budget::new(MOST_INDEX_BYTES))?.sessions,
            Read::Missing => Vec::new(),
        };
        let mut out = 0;
        for session in sessions {
            let files = [
                (record(session)?, Loading::Record { session }),
                (transcript(session)?, Loading::Transcript { session }),
            ];
            for (file, loading) in files {
                turn.to(path::<Files>(())).ask(&ReadFile { namespace: Namespace::Save, path: file }, &loading)?;
                out += 1;
            }
        }
        let reading = Reading { records: BTreeMap::new(), transcripts: BTreeMap::new(), out };
        if out == 0 {
            return self.found(turn, reading, waiting);
        }
        self.phase = Phase::Reading { reading, waiting };

        Ok(())
    }

    /// One more read of `reading` is answered: once none is out, the sessions are found.
    fn answered(
        &mut self,
        turn: &mut Turn<'_, Self, Acts>,
        mut reading: Reading,
        waiting: Vec<Waiting>,
    ) -> Result<(), Fault> {
        reading.out = reading.out.saturating_sub(1);
        if reading.out > 0 {
            self.phase = Phase::Reading { reading, waiting };
            return Ok(());
        }

        self.found(turn, reading, waiting)
    }

    /// Every session `reading` found, each held as stopped until the containers carrying the hub's label are listed.
    fn found(&mut self, turn: &mut Turn<'_, Self, Acts>, reading: Reading, waiting: Vec<Waiting>) -> Result<(), Fault> {
        let Some(docker) = turn.found(&path::<Docker>(())) else {
            turn.log(Level::Error, "no Docker client is listed before the sessions");
            return Err(Fault::new(&Unfound::Docker)?);
        };
        let Reading { records, mut transcripts, .. } = reading;
        let mut recorded = Vec::new();
        for (session, record) in records {
            if let Standing::Running { container, run } = &record.kept {
                recorded.push(Recorded { session, container: container.clone(), run: *run });
            }
            let life = match record.kept {
                Standing::Archived => Life::Archived,
                Standing::Running { .. } | Standing::Stopped => Life::Stopped,
            };
            let transcript = transcripts.remove(&session).unwrap_or_default();
            let Record { name, conversation, runs, .. } = record;
            self.held.insert(session, Session { name, conversation, runs, transcript, life });
        }

        self.watching.open(turn, docker.protocol(), Events { label: self.settings.hub_label() }, ())?;
        let due = self.due(turn, 0);
        turn.to(path::<Docker>(())).within(due).ask(&List { label: self.settings.hub_label() }, &Reconciling)?;
        self.phase = Phase::Listing { docker, recorded, waiting };

        Ok(())
    }
}

impl Handles<Answer<Listed, Reconciling>> for Sessions {
    type Sends = Acts;

    /// The containers carrying the hub's label: each a record says may run is re-attached when it runs, and noted
    /// lost when it does not; each with no record is reported.
    fn handle(
        &mut self,
        (answered, Reconciling): (Answered<Listed>, Reconciling),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let Phase::Listing { docker, recorded, waiting } =
            mem::replace(&mut self.phase, Phase::Indexing { waiting: Vec::new() })
        else {
            return Ok(());
        };
        self.phase = Phase::Ready { docker };
        let listings = match answered {
            Answered::Yes(Listed::Containers(listings)) => listings,
            Answered::Yes(Listed::Failed(why)) => {
                turn.log(Level::Error, format_args!("the sessions' containers could not be listed: {why:?}"));
                Vec::new()
            }
            Answered::No(why) => {
                turn.log(Level::Error, format_args!("the sessions' containers went unlisted: {why:?}"));
                Vec::new()
            }
        };
        let known: Vec<ContainerId> = recorded.iter().map(|recorded| recorded.container.clone()).collect();
        for Recorded { session, container, run } in recorded {
            let listing = listings.iter().find(|listing| listing.id == container);
            let runs = listing.is_some_and(|listing| listing.phase == DockerPhase::Running);
            if runs {
                self.resume(turn, session, container, run)?;
            } else {
                self.note(session, Entry::Hub(Note::Stopped { run, why: Ended::Lost }));
                if listing.is_some() {
                    self.remove(turn, session, container, run)?;
                }
            }
        }
        for Listing { id, name, .. } in listings {
            if !known.contains(&id) {
                turn.log(Level::Warn, format_args!("a container carries the hub's label with no session: {name}"));
                self.strays.push(name);
            }
        }

        for asked in waiting {
            self.take(turn, asked)?;
        }

        self.flush(turn)
    }
}
