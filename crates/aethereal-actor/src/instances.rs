//! The instances a part's actors run their code in, which the engine carries with the part and never looks into
//! (ADR-0015).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::{Any, TypeId};

/// The instances one part's actors run their code in: each loaded by the host of the first actor that wanted it, and
/// reused by every other actor of the part that wants the same (ADR-0015).
///
/// The engine keeps one for each part, moves it with the part, and lends it to every batch and turn of the part's
/// actors, as it lends them the table; what is in it is the hosts' alone. A part is run by one worker at a time, and
/// that worker runs one of its actors at a time, so what is lent is lent whole, and nothing in it is locked.
///
/// A host keys what it loads by a number of its own, under the type it loads, so no host reaches another's.
#[derive(Default)]
pub struct Instances {
    loaded: Vec<Loaded>,
}

/// One instance a host loaded: its type, the key the host gave it, and the instance.
struct Loaded {
    host: TypeId,
    key: u64,
    instance: Box<dyn Any + Send>,
}

impl Instances {
    /// The instance of type `I` kept under `key`, loaded with `load` when there is none yet.
    ///
    /// # Panics
    ///
    /// Never: an instance is kept under its own type, so it is always read back as that type.
    pub fn get_or_load<I: Any + Send>(&mut self, key: u64, load: impl FnOnce() -> I) -> &mut I {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        let at = found.unwrap_or_else(|| {
            self.loaded.push(Loaded { host, key, instance: Box::new(load()) });
            self.loaded.len() - 1
        });
        let Some(instance) = self.loaded[at].instance.downcast_mut::<I>() else {
            panic!("an instance is kept under its own type");
        };

        instance
    }
}

impl Loaded {
    /// Whether it is the instance of type `host` kept under `key`.
    fn is(&self, host: TypeId, key: u64) -> bool {
        let is_host = self.host == host;
        let is_key = self.key == key;

        is_host && is_key
    }
}
