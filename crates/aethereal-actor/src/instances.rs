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

    /// Drops the instance of type `I` kept under `key`, for a host whose last actor that ran in it has ended: the
    /// next actor of the part that wants it loads a new one. Nothing when there is none.
    pub fn drop_instance<I: Any + Send>(&mut self, key: u64) {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        if let Some(at) = found {
            self.loaded.swap_remove(at);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An instance of one host, numbered by the load that made it.
    struct Made(u32);

    /// An instance of another host.
    struct Other(u32);

    #[test]
    fn an_instance_is_loaded_once_for_its_key_and_type() {
        // Catches a lookup that misses what it loaded and loads again, a key or a host's type read as another's, and
        // a drop that takes the wrong instance or none.
        let mut instances = Instances::default();
        let mut loads = 0;
        let mut load = || {
            loads += 1;
            Made(loads)
        };

        let first = instances.get_or_load(1, &mut load).0;
        let again = instances.get_or_load(1, &mut load).0;
        let other_key = instances.get_or_load(2, &mut load).0;
        let other_host = instances.get_or_load(1, || Other(99)).0;
        instances.drop_instance::<Made>(1);
        let reloaded = instances.get_or_load(1, &mut load).0;
        let kept = instances.get_or_load(2, &mut load).0;

        assert_eq!((first, again, other_key, other_host, reloaded, kept), (1, 1, 2, 99, 3, 2));
    }
}
