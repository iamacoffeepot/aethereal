//! A module of `Code` published again while the program runs: read, taken in as code, and published as the head of
//! every kind it makes (ADR-0054).

use super::{Fetching, Library, ReadingModule, Stage};
use crate::{ModulePublished, Numbered, PublishModule, Unloaded};
use aethereal_actor::Level;
use aethereal_actor::{Answered, Asks, Bytes, Fault, Handles, Path, ToSender, Turn, path};
use aethereal_data::{FilePath, to_bytes};
use aethereal_files::{FileRead, Files, Namespace, ReadFile};

/// Reads the module, once `Code` is published: answered once it is read.
impl Handles<PublishModule> for Library {
    type Sends = (Asks<Path<Files>, ReadFile, ReadingModule>, ToSender<ModulePublished>);

    fn handle(&mut self, publish: PublishModule, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_open = matches!(self.stage, Stage::Open);
        if !is_open {
            turn.reply(&ModulePublished::Unopened)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        let read = ReadFile { namespace: Namespace::Code, path: publish.path.clone() };
        turn.to(path::<Files>(())).ask(&read, &ReadingModule::Publishing { number })?;
        self.fetching.insert(number, Fetching { held, path: publish.path });

        Ok(())
    }
}

impl Library {
    /// The module read for the publish asked under `number`, as `answered` says: published as the head of every kind
    /// it makes, and the publish answered.
    pub(super) fn read_to_publish(
        &mut self,
        number: u64,
        answered: Answered<FileRead>,
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Fetching { held, path }) = self.fetching.remove(&number) else {
            return Ok(());
        };
        let published = match answered {
            Answered::Yes(FileRead::Read { bytes }) => self.publish_module(&path, bytes, turn)?,
            Answered::Yes(FileRead::Missing) => ModulePublished::Unloaded(Unloaded::Missing),
            Answered::Yes(FileRead::Unavailable) => ModulePublished::Unloaded(Unloaded::Unavailable),
            Answered::Yes(FileRead::Failed) | Answered::No(_) => ModulePublished::Unloaded(Unloaded::Unread),
        };
        turn.answer(held, &published)?;

        Ok(())
    }

    /// Takes `module`, the module of `Code` at `path`, in as code and publishes it as the head of every kind it
    /// makes. Why the guests' host refused it, or the engine a publish, is logged as the library's.
    ///
    /// # Errors
    ///
    /// Why the kinds it publishes did not encode.
    fn publish_module<S>(
        &mut self,
        path: &FilePath,
        module: Bytes,
        turn: &mut Turn<'_, Self, S>,
    ) -> Result<ModulePublished, Fault> {
        let kinds = match turn.load_code(&module) {
            Ok(kinds) => kinds,
            Err(unfit) => {
                turn.log(Level::Error, format_args!("`{path}` in `Code` is no module this engine runs: {unfit:?}"));
                return Ok(ModulePublished::Unloaded(Unloaded::Unfit));
            }
        };
        if kinds.is_empty() {
            return Ok(ModulePublished::Unloaded(Unloaded::MakesNothing));
        }
        let mut numbered = Vec::new();
        for kind in kinds {
            let publish = match self.publish(&kind, &module, turn) {
                Ok(publish) => publish,
                Err(refused) => {
                    turn.log(Level::Error, format_args!("`{path}` was not published as `{kind}`: {refused:?}"));
                    return Ok(ModulePublished::Unloaded(Unloaded::Unfit));
                }
            };
            numbered.push(Numbered { kind, publish });
        }
        let Ok(kinds) = turn.put(to_bytes(&numbered)?) else {
            turn.log(Level::Error, format_args!("the kinds `{path}` makes did not fit in the engine's blobs"));
            return Ok(ModulePublished::Unloaded(Unloaded::Unread));
        };

        Ok(ModulePublished::Published { module, kinds })
    }
}
