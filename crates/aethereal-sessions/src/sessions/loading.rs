//! How the capability begins (ADR-0057): it reads its sessions through `Files`, the index and then each session's
//! record and transcript, follows Docker's events for its label, and only then lists the containers carrying it, so
//! nothing that happens to one falls between the two. It reads only those two facts, what each record says and what
//! Docker lists, never when anything was written. A container a record names that carries the bootstrap label is a
//! run's bootstrap container, in whatever state Docker lists it: it is followed and judged from Docker, as the hub
//! before would have judged it, and one the record names that is not listed is a failed start. A Claude Code container
//! a record says may exist is adopted when it runs, and removed when it does not, the session left hibernating, its
//! container lost, once it is gone; a record that says its session is awake in a Claude Code container Docker does not
//! list leaves it hibernating, its container lost; a record that says its run starts, of which Docker lists no
//! container, leaves it hibernating after a failed start, as only the hub removes a container, so the hub before
//! stopped before Claude Code ran in that run; and each container carrying the label that no record names is reported,
//! never removed. A container it adopts has its bearer read back first, from the environment the hub before gave it,
//! as Docker inspects it, and held for the session whose record names it, never one its environment names, so it
//! reaches the hub as it did; one whose environment holds none is adopted with none, and its calls refused. Asks wait
//! until every container it adopts is. A record that says its session hibernates lost reads back so, and one that says
//! its last start failed reads back saying why, holding the lines that waited for it still. A list Docker does not
//! answer with its containers fails the capability before anything is noted or written: which sessions still run is
//! unknown, and every record is left as it is for a hub that starts again to list them.

use super::asked::Waiting;
use super::bearers::Bearer;
use super::container::{self, BEARER_VARIABLE};
use super::record::{Record, Standing};
use super::saves::{Index, Loading, record, transcript};
use super::session::{Gone, Life, Session, Stage};
use super::settings::BOOTSTRAP_LABEL;
use super::transcript::{Ended, Entry, Note, Transcript};
use super::{Adoption, Asking, Named, Phase, Reconciling, Recorded, Sessions, Unfound};
use crate::{
    Docker, DockerError, Events, List, Listed, Listing, Phase as DockerPhase, ReadVariable, Variable, VariableRead,
};
use aethereal_actor::{ActorRef, Answered, Fault, Level, Turn, path};
use aethereal_data::{Budget, Plain, from_bytes};
use aethereal_files::{FileRead, Files, Namespace, ReadFile};
use core::{fmt, mem};
use std::collections::BTreeMap;

/// The most bytes the index holds as it is read back: past a million sessions' numbers.
const MOST_INDEX_BYTES: usize = 4 << 20;

/// The most bytes a session's record holds as it is read back.
const MOST_RECORD_BYTES: usize = 64 * 1024;

/// Why a session's start failed, as a hub that starts again finds its record saying a run starts and Docker lists no
/// container of that run: only the hub removes one, so the hub before stopped before it made one, or after it removed
/// the run's bootstrap container and before it made Claude Code's, and Claude Code never ran in that run.
const UNFINISHED: &str = "the hub stopped before the session's start finished, so Claude Code never ran in it";

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
    /// listed: one whose record says it was lost as lost, and one whose record says it was awake or starting as lost
    /// too, until its container is found and adopted or judged, or found missing. Each is held whatever it stood as,
    /// archived ones among them, so any reads in full with no container.
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
                Standing::Bootstrapping { container, run } => {
                    recorded.push(Recorded { session, container: Named::Bootstrap(container.clone()), run: *run });
                }
                Standing::Starting { run } => {
                    let claude = container::name(&self.settings, session, *run);
                    let bootstrap = container::bootstrap_name(&self.settings, session, *run);
                    recorded.push(Recorded { session, container: Named::Starting { claude, bootstrap }, run: *run });
                }
                Standing::Hibernating | Standing::Lost | Standing::Unstarted { .. } | Standing::Archived => {}
            }
            let life = match record.kept {
                Standing::Archived => Life::Archived,
                Standing::Hibernating => Life::Hibernating { gone: Gone::Removed },
                Standing::Unstarted { unread, why } => Life::Hibernating { gone: Gone::Unstarted { unread, why } },
                Standing::Lost
                | Standing::Awake { .. }
                | Standing::Starting { .. }
                | Standing::Bootstrapping { .. } => Life::Hibernating { gone: Gone::Lost },
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

    /// The containers carrying the hub's label: a bootstrap container a record names, by its label, is adopted, to be
    /// followed and judged in whatever state it is, and one a record names that is not listed is a failed start; a
    /// Claude Code container a record says may exist is adopted when it runs, and noted lost and removed when it does
    /// not; a record whose Claude Code container is named by its id and not listed is noted lost; a record whose run
    /// starts, of which no container is listed, is a failed start, as Claude Code never ran in it; and each no record
    /// names is reported. Each container adopted has its bearer read back first, and asks wait until each is adopted.
    /// A list that fails fails the capability, [`Unfound::Unlisted`], before any session is noted or any record
    /// written, as which containers exist is unknown and none may be taken as gone.
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
        for listing in &listings {
            let is_named = recorded.iter().any(|recorded| recorded.names(listing));
            if !is_named {
                let name = &listing.name;
                turn.log(Level::Warn, format_args!("a container carries the hub's label with no session: {name}"));
                self.strays.push(name.clone());
            }
        }
        let mut adopting = Vec::new();
        for Recorded { session, container, run } in recorded {
            let found = listings.iter().find(|listing| container.names(listing));
            match found {
                Some(listing) if listing.carries(BOOTSTRAP_LABEL) => {
                    let container = listing.id.clone();
                    adopting.push(Adoption { session, container, run, stage: Stage::Bootstrap });
                }
                Some(Listing { id, phase: DockerPhase::Running, .. }) => {
                    adopting.push(Adoption { session, container: id.clone(), run, stage: Stage::Claude });
                }
                Some(Listing { id, .. }) => {
                    self.note(turn, session, Entry::Hub(Note::Stopped { run, why: Ended::Lost }));
                    self.retire(turn, session, id.clone(), run, Gone::Lost)?;
                }
                None => match container {
                    Named::Made(_) => {
                        self.note(turn, session, Entry::Hub(Note::Stopped { run, why: Ended::Lost }));
                    }
                    Named::Bootstrap(_) => self.bootstrap_gone(turn, session, run),
                    Named::Starting { .. } => self.left_unstarted(turn, session, run, String::from(UNFINISHED)),
                },
            }
        }
        if adopting.is_empty() {
            return self.ready(turn, docker, waiting);
        }
        let due = self.due(turn, 0);
        for Adoption { session, container, run, .. } in &adopting {
            let read = ReadVariable { container: container.clone(), name: String::from(BEARER_VARIABLE) };
            let asking = Asking { session: *session, run: *run };
            turn.to(path::<Docker>(())).within(due).ask(&read, &asking)?;
        }
        self.phase = Phase::Admitting { docker, adopting, waiting };

        self.flush(turn)
    }

    /// The bearer of the container of `session`'s run `run` that the capability adopts, read back from its
    /// environment: it is held for `session`, whose record names the container, whatever the environment says of
    /// itself, and the container adopted; one that reads no bearer is adopted with none, so its calls are refused, and
    /// logged without what Docker answered. Once every container is adopted, the capability runs, and the asks that
    /// waited are answered. A read of no container being adopted is let go.
    pub(super) fn handle_variable_read(
        &mut self,
        (answered, Asking { session, run }): (Answered<VariableRead>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Phase::Admitting { adopting, .. } = &mut self.phase else {
            return Ok(());
        };
        let Some(at) = adopting.iter().position(|adoption| adoption.is_of(session, run)) else {
            return Ok(());
        };
        let Adoption { container, stage, .. } = adopting.swap_remove(at);
        match bearer(answered) {
            Ok(bearer) => self.bearers.admit(bearer, session),
            Err(unborne) => turn.log(
                Level::Warn,
                format_args!(
                    "session {session}'s container is adopted with no bearer, so its calls are refused: {unborne}"
                ),
            ),
        }
        match stage {
            Stage::Bootstrap => self.adopt_bootstrap(turn, session, container, run)?,
            Stage::Claude => self.adopt(turn, session, container, run)?,
        }

        let is_admitted = matches!(&self.phase, Phase::Admitting { adopting, .. } if adopting.is_empty());
        if !is_admitted {
            return self.flush(turn);
        }
        let Phase::Admitting { docker, waiting, .. } =
            mem::replace(&mut self.phase, Phase::Indexing { waiting: Vec::new() })
        else {
            return self.flush(turn);
        };

        self.ready(turn, docker, waiting)
    }

    /// The capability has found its sessions and their containers: it runs, and answers each ask that waited.
    fn ready(
        &mut self,
        turn: &mut Turn<'_, Self>,
        docker: ActorRef<Docker>,
        waiting: Vec<Waiting>,
    ) -> Result<(), Fault> {
        self.phase = Phase::Ready { docker };
        for asked in waiting {
            self.take(turn, asked)?;
        }

        self.flush(turn)
    }

    /// The start of `session`'s run `run` failed, as `why` says, which a hub that starts again finds as it lists the
    /// run's containers: the session hibernates saying so, holding no line, as what waited for the run went with the
    /// hub before, and is never told it was lost, as Claude Code never ran in the run.
    pub(super) fn left_unstarted(&mut self, turn: &Turn<'_, Self>, session: u32, run: u32, why: String) {
        let Some(held) = self.held.get_mut(&session) else {
            return;
        };
        held.life = Life::Hibernating { gone: Gone::Unstarted { unread: Vec::new(), why: why.clone() } };
        self.note(turn, session, Entry::Hub(Note::Unstarted { run, why }));
    }
}

/// Why a container the capability adopts reads no bearer: its environment sets none, or one that is no bearer, or
/// Docker did not read it. It writes nothing Docker answered of the environment.
enum Unborne {
    Unset,
    Unreadable,
    Unread(DockerError),
}

impl fmt::Display for Unborne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unset => write!(f, "its environment sets no {BEARER_VARIABLE}"),
            Self::Unreadable => write!(f, "its environment's {BEARER_VARIABLE} is no bearer"),
            Self::Unread(why) => write!(f, "Docker did not read its environment: {why:?}"),
        }
    }
}

/// The bearer a read of a container's environment answered.
///
/// # Errors
///
/// It sets none, or one that is no bearer, or the read failed.
fn bearer(answered: Answered<VariableRead>) -> Result<Bearer, Unborne> {
    match answered {
        Answered::Yes(VariableRead::Set(Variable { value, .. })) => Bearer::of_token(&value).ok_or(Unborne::Unreadable),
        Answered::Yes(VariableRead::Unset) => Err(Unborne::Unset),
        Answered::Yes(VariableRead::Failed(why)) => Err(Unborne::Unread(why)),
        Answered::No(why) => Err(Unborne::Unread(DockerError::Unanswered(why))),
    }
}

impl Adoption {
    /// Whether it is the container of `session`'s run `run`.
    const fn is_of(&self, session: u32, run: u32) -> bool {
        self.session == session && self.run == run
    }
}

impl Recorded {
    /// Whether `listing` is the container this record names.
    fn names(&self, listing: &Listing) -> bool {
        self.container.names(listing)
    }
}

impl Named {
    /// Whether `listing` is the container this names: by its id, or by either of its run's names.
    fn names(&self, listing: &Listing) -> bool {
        match self {
            Self::Made(id) | Self::Bootstrap(id) => listing.id == *id,
            Self::Starting { claude, bootstrap } => listing.name == *claude || listing.name == *bootstrap,
        }
    }
}
