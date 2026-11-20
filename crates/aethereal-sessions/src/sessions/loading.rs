//! How the capability begins (ADR-0057): it reads its sessions through `Files`, the index and then each session's
//! record and transcript, follows Docker's events for its label, and only then lists the containers carrying it, so
//! nothing that happens to one falls between the two. It reads only those two facts, what each record says and what
//! Docker lists, never when anything was written: a container a record says may exist is adopted when it runs, and
//! removed when it does not, the session left hibernating, its container lost, once it is gone; a record whose
//! container is not listed leaves its session hibernating, its container lost; and each container carrying the label
//! that no record names is reported, never removed. A record that says its session hibernates lost reads back so, and
//! one that says it holds lines Claude Code never read, as its last start failed, holds them still. A
//! list Docker does not answer with its containers fails the capability before anything is noted or written: which
//! sessions still run is unknown, and every record is left as it is for a hub that starts again to list them.

use super::asked::Waiting;
use super::container;
use super::record::{Record, Standing};
use super::saves::{Index, Loading, record, transcript};
use super::session::{Gone, Life, Session};
use super::transcript::{Ended, Entry, Note, Transcript};
use super::{Named, Phase, Reconciling, Recorded, Sessions, Unfound};
use crate::{Docker, Events, List, Listed, Listing, Phase as DockerPhase};
use aethereal_actor::{Answered, Fault, Level, Turn, path};
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

impl Sessions {
    /// A file of the sessions read: the index asks for each session's record and transcript, and once every one is
    /// answered, the sessions are held as hibernating until the containers carrying the hub's label are listed.
    pub(super) fn handle_file_read(
        &mut self,
        (answered, loading): (Answered<FileRead>, Loading),
        turn: &mut Turn<'_, Self>,
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
fn unread(turn: &mut Turn<'_, Sessions>, loading: Loading, why: &str) -> Result<(), Fault> {
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
    fn indexed(&mut self, turn: &mut Turn<'_, Self>, read: &Read, waiting: Vec<Waiting>) -> Result<(), Fault> {
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
        turn: &mut Turn<'_, Self>,
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

    /// Every session `reading` found, each held as hibernating until the containers carrying the hub's label are
    /// listed: one whose record says it was lost as lost, and one whose record says it was awake as lost too, unless
    /// its container is found running and adopted. Each is held whatever it stood as, archived ones among them, so any
    /// reads in full with no container.
    fn found(&mut self, turn: &mut Turn<'_, Self>, reading: Reading, waiting: Vec<Waiting>) -> Result<(), Fault> {
        let Some(docker) = turn.found(&path::<Docker>(())) else {
            turn.log(Level::Error, "no Docker client is listed before the sessions");
            return Err(Fault::new(&Unfound::Docker)?);
        };
        let Reading { records, mut transcripts, .. } = reading;
        let mut recorded = Vec::new();
        for (session, record) in records {
            match &record.kept {
                Standing::Awake { container, run } => {
                    recorded.push(Recorded { session, container: Named::Made(container.clone()), run: *run });
                }
                Standing::Starting { run } => {
                    let name = container::name(&self.settings, session, *run);
                    recorded.push(Recorded { session, container: Named::Starting(name), run: *run });
                }
                Standing::Hibernating | Standing::Lost | Standing::Unstarted { .. } | Standing::Archived => {}
            }
            let life = match record.kept {
                Standing::Archived => Life::Archived,
                Standing::Hibernating => Life::Hibernating { gone: Gone::Removed },
                Standing::Unstarted { unread } => Life::Hibernating { gone: Gone::Unstarted { unread } },
                Standing::Lost | Standing::Awake { .. } | Standing::Starting { .. } => {
                    Life::Hibernating { gone: Gone::Lost }
                }
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

    /// The containers carrying the hub's label: each a record says may exist is adopted when it runs, and noted lost
    /// and removed when it does not; a record whose container is not listed is noted lost; and each no record names is
    /// reported. A list that fails fails the capability, [`Unfound::Unlisted`], before any session is noted or any
    /// record written, as which containers exist is unknown and none may be taken as gone.
    pub(super) fn handle_listed(
        &mut self,
        (answered, Reconciling): (Answered<Listed>, Reconciling),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Phase::Listing { docker, recorded, waiting } =
            mem::replace(&mut self.phase, Phase::Indexing { waiting: Vec::new() })
        else {
            return Ok(());
        };
        let listings = match answered {
            Answered::Yes(Listed::Containers(listings)) => listings,
            Answered::Yes(Listed::Failed(why)) => {
                turn.log(Level::Error, format_args!("the sessions' containers could not be listed: {why:?}"));
                return Err(Fault::new(&Unfound::Unlisted)?);
            }
            Answered::No(why) => {
                turn.log(Level::Error, format_args!("the sessions' containers went unlisted: {why:?}"));
                return Err(Fault::new(&Unfound::Unlisted)?);
            }
        };
        self.phase = Phase::Ready { docker };
        for listing in &listings {
            let is_named = recorded.iter().any(|recorded| recorded.names(listing));
            if !is_named {
                let name = &listing.name;
                turn.log(Level::Warn, format_args!("a container carries the hub's label with no session: {name}"));
                self.strays.push(name.clone());
            }
        }
        for Recorded { session, container, run } in recorded {
            let found = listings.iter().find(|listing| container.names(listing));
            match found {
                Some(Listing { id, phase: DockerPhase::Running, .. }) => self.adopt(turn, session, id.clone(), run)?,
                Some(Listing { id, .. }) => {
                    self.note(turn, session, Entry::Hub(Note::Stopped { run, why: Ended::Lost }));
                    self.retire(turn, session, id.clone(), run, Gone::Lost)?;
                }
                None => {
                    self.note(turn, session, Entry::Hub(Note::Stopped { run, why: Ended::Lost }));
                }
            }
        }

        for asked in waiting {
            self.take(turn, asked)?;
        }

        self.flush(turn)
    }
}

impl Recorded {
    /// Whether `listing` is the container this record names.
    fn names(&self, listing: &Listing) -> bool {
        self.container.names(listing)
    }
}

impl Named {
    /// Whether `listing` is the container this names: by its id, or by its name.
    fn names(&self, listing: &Listing) -> bool {
        match self {
            Self::Made(id) => listing.id == *id,
            Self::Starting(name) => listing.name == *name,
        }
    }
}
