//! The library as the program starts: `Code` listed, each module read, and every kind each makes published, or the
//! program stopped naming the modules that kept it from starting (ADR-0054).

use super::{Deferred, Library, Listing, Module, Reading, ReadingModule, Stage};
use crate::Defaulted;
use aethereal_actor::{Answer, Answered, Asks, Fault, Handles, Level, Path, Turn, path};
use aethereal_data::{FilePath, KindName};
use aethereal_files::{FileRead, Files, FilesListed, Namespace, ReadFile};
use std::collections::BTreeMap;
use std::mem;

/// `Code` listed: each module read, or `Code` published at once when it holds none.
impl Handles<Answer<FilesListed, Listing>> for Library {
    type Sends = (Asks<Path<Files>, ReadFile, ReadingModule>,);

    fn handle(
        &mut self,
        (answered, Listing): (Answered<FilesListed>, Listing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Stage::Listing(deferred) = mem::replace(&mut self.stage, Stage::Stopped) else {
            return Ok(());
        };
        let paths = match answered {
            Answered::Yes(FilesListed::Listed { paths }) => paths,
            // A program with no `Code`, or a host with nowhere to keep one, runs no module of its own.
            Answered::Yes(FilesListed::Missing | FilesListed::Unavailable) => Vec::new(),
            Answered::Yes(FilesListed::NotListed | FilesListed::Failed) => {
                return self.stop(turn, "`Code` could not be listed", deferred);
            }
            Answered::No(why) => return self.stop(turn, &format!("`Files` did not list `Code`: {why:?}"), deferred),
        };
        if paths.is_empty() {
            return self.open(Vec::new(), deferred, turn);
        }
        let left = u32::try_from(paths.len()).unwrap_or(u32::MAX);
        for module in paths {
            let read = ReadFile { namespace: Namespace::Code, path: module.clone() };
            turn.to(path::<Files>(())).ask(&read, &ReadingModule::Starting { path: module })?;
        }
        self.stage = Stage::Reading(Reading { read: Vec::new(), left, deferred });

        Ok(())
    }
}

impl Library {
    /// The module of `Code` at `path`, read as the program starts, as `answered` says: `Code` published once the last
    /// is read.
    pub(super) fn read_as_starting(
        &mut self,
        path: FilePath,
        answered: Answered<FileRead>,
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Stage::Reading(reading) = &mut self.stage else {
            return Ok(());
        };
        let Answered::Yes(FileRead::Read { bytes }) = answered else {
            let deferred = mem::take(&mut reading.deferred);
            return self.stop(turn, &format!("`{path}` in `Code` could not be read"), deferred);
        };
        reading.read.push(Module { path, blob: bytes });
        reading.left = reading.left.saturating_sub(1);
        let is_read = reading.left == 0;
        if !is_read {
            return Ok(());
        }
        let (read, deferred) = (mem::take(&mut reading.read), mem::take(&mut reading.deferred));

        self.open(read, deferred, turn)
    }

    /// Publishes every kind the modules `read` make, and opens: the defaults `deferred` held meanwhile are taken, in
    /// the order asked. Two modules that make one kind, or one the guests' host refuses, stop the program instead.
    fn open<S>(
        &mut self,
        mut read: Vec<Module>,
        deferred: Vec<Deferred>,
        turn: &mut Turn<'_, Self, S>,
    ) -> Result<(), Fault> {
        read.sort_by(|a, b| a.path.cmp(&b.path));
        let mut made: BTreeMap<KindName, usize> = BTreeMap::new();
        for (at, module) in read.iter().enumerate() {
            let kinds = match turn.load_code(&module.blob) {
                Ok(kinds) => kinds,
                Err(unfit) => {
                    let why = format!("`{}` in `Code` is no module this engine runs: {unfit:?}", module.path);
                    return self.stop(turn, &why, deferred);
                }
            };
            for kind in kinds {
                if let Some(&first) = made.get(&kind) {
                    let first = &read[first].path;
                    let why = format!("`{first}` and `{}` in `Code` both make `{kind}`", module.path);
                    return self.stop(turn, &why, deferred);
                }
                made.insert(kind, at);
            }
        }
        for (kind, at) in &made {
            if let Err(refused) = self.publish(kind, &read[*at].blob, turn) {
                let why = format!("`{}` in `Code` was not published as `{kind}`: {refused:?}", read[*at].path);
                return self.stop(turn, &why, deferred);
            }
        }
        self.stage = Stage::Open;

        for Deferred { held, modules } in deferred {
            let defaulted = self.take_defaults(&modules, turn);
            turn.answer(held, &defaulted)?;
        }

        Ok(())
    }

    /// Stops the program as it starts, for `why`, logged as the library's own: the defaults `deferred` held meanwhile
    /// are answered that it stopped.
    fn stop<S>(&mut self, turn: &mut Turn<'_, Self, S>, why: &str, deferred: Vec<Deferred>) -> Result<(), Fault> {
        turn.log(Level::Error, format_args!("{why}, so the program stops"));
        turn.quit();
        self.stage = Stage::Stopped;
        for Deferred { held, .. } in deferred {
            turn.answer(held, &Defaulted::Stopped)?;
        }

        Ok(())
    }
}
