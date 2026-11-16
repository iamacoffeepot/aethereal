//! How the capability begins (ADR-0057): it reads its sessions from disk, follows Docker's events for its label, and
//! only then lists the containers carrying it, so nothing that happens to one falls between the two. Each container a
//! record says may still run is re-attached when it does, and each it has no record of is reported, never removed.

use super::disk::{Found, Kept, Loading};
use super::record::Standing;
use super::session::{Life, Session};
use super::transcript::{Ended, Entry, Note, Transcript};
use super::{Acts, Phase, Reconciling, Recorded, Sessions, Unfound};
use crate::{ContainerId, Docker, Events, List, Listed, Listing, Phase as DockerPhase};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Turn, path};
use core::mem;

impl Handles<Answer<Found, Loading>> for Sessions {
    type Sends = Acts;

    /// The sessions read: each held as stopped until the containers carrying the hub's label are listed.
    fn handle(
        &mut self,
        (answered, Loading): (Answered<Found>, Loading),
        turn: &mut Turn<'_, Self, Acts>,
    ) -> Result<(), Fault> {
        let Phase::Loading { waiting } = mem::replace(&mut self.phase, Phase::Loading { waiting: Vec::new() }) else {
            return Ok(());
        };
        let Answered::Yes(Found::Read(kept)) = answered else {
            turn.log(Level::Error, format_args!("the sessions could not be read: {answered:?}"));
            return Err(Fault::new(&Unfound::Disk)?);
        };
        let Some(docker) = turn.found(&path::<Docker>(())) else {
            turn.log(Level::Error, "no Docker client is listed before the sessions");
            return Err(Fault::new(&Unfound::Docker)?);
        };
        let mut recorded = Vec::new();
        for Kept { session, record, transcript } in kept {
            if let Standing::Running { container, run } = &record.kept {
                recorded.push(Recorded { session, container: container.clone(), run: *run });
            }
            let life = match record.kept {
                Standing::Archived => Life::Archived,
                Standing::Running { .. } | Standing::Stopped => Life::Stopped,
            };
            let transcript = Transcript::read(turn.read(&transcript));
            let held =
                Session { name: record.name, conversation: record.conversation, runs: record.runs, transcript, life };
            self.held.insert(session, held);
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
            mem::replace(&mut self.phase, Phase::Loading { waiting: Vec::new() })
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
