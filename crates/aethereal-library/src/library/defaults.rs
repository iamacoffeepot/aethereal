//! Defaults: the modules a system ships, published only for the kinds that have no head (ADR-0054).

use super::{Deferred, Library, Reading, Stage};
use crate::{Defaulted, Defaults};
use aethereal_actor::{Bytes, Fault, Handles, Level, ToSender, Turn};
use aethereal_data::KindName;
use std::collections::BTreeMap;

/// Taken at once while the library is open; held until `Code` is published while it starts, so `Code` comes first;
/// and answered that it stopped once it has.
impl Handles<Defaults> for Library {
    type Sends = (ToSender<Defaulted>,);

    fn handle(&mut self, Defaults { modules }: Defaults, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let deferred = match &mut self.stage {
            Stage::Listing(deferred) | Stage::Reading(Reading { deferred, .. }) => deferred,
            Stage::Open => {
                let defaulted = self.take_defaults(&modules, turn);
                turn.reply(&defaulted)?;
                return Ok(());
            }
            Stage::Stopped => {
                turn.reply(&Defaulted::Stopped)?;
                return Ok(());
            }
        };
        deferred.push(Deferred { held: turn.hold()?, modules });

        Ok(())
    }
}

impl Library {
    /// Takes `modules` as defaults, all or none: each is taken in as code, and each kind they make is published from
    /// the module that makes it when it has no head, and keeps its head when it has one.
    pub(super) fn take_defaults<S>(&mut self, modules: &[Bytes], turn: &mut Turn<'_, Self, S>) -> Defaulted {
        let mut made: BTreeMap<KindName, usize> = BTreeMap::new();
        for (at, module) in modules.iter().enumerate() {
            let module_number = u32::try_from(at).unwrap_or(u32::MAX);
            let kinds = match turn.load_code(module) {
                Ok(kinds) => kinds,
                Err(unfit) => {
                    turn.log(
                        Level::Error,
                        format_args!("default {module_number} is no module this engine runs: {unfit:?}"),
                    );
                    return Defaulted::Unfit { module: module_number };
                }
            };
            for kind in kinds {
                let is_twice = made.insert(kind.clone(), at).is_some();
                if is_twice {
                    return Defaulted::Twice { kind };
                }
            }
        }

        let (mut published, mut kept) = (0, 0);
        for (kind, at) in made {
            if self.has_head(&kind) {
                kept += 1;
                continue;
            }
            if let Err(refused) = self.publish(&kind, &modules[at], turn) {
                turn.log(Level::Error, format_args!("default {at} was not published as `{kind}`: {refused:?}"));
                return Defaulted::Unfit { module: u32::try_from(at).unwrap_or(u32::MAX) };
            }
            published += 1;
        }

        Defaulted::Taken { published, kept }
    }
}
