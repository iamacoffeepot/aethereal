//! The instances a part's actors run their code in, which the engine carries with the part and never looks into
//! (ADR-0015).

use crate::{Declared, Guests, Links, NoGuests, Refused, Seated};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::{Any, TypeId};
use core::mem;
use core::ops::{Deref, DerefMut};

/// The instances one part's actors run their code in: each loaded by the host of the first actor that wanted it, and
/// reused by every other actor of the part that wants the same (ADR-0015).
///
/// The engine keeps one for each part, moves it with the part, and lends it to every batch and turn of the part's
/// actors, as it lends them the table; what is in it is the hosts' alone. A part is run by one worker at a time, and
/// that worker runs one of its actors at a time, so what is lent is lent whole, and nothing in it is locked.
///
/// A host keys what it loads by a number of its own, under the type it loads, so no host reaches another's. Each
/// instance loaded is handed a number no instance loaded into them before took, so a host tells an instance made
/// again under a key, as a trap dropped the one before it, from the one it replaced (ADR-0015, ADR-0054).
///
/// It also carries the engine's [`Guests`], the code it loads from, so a turn
/// that spawns a child from a blob reaches it through what it is lent, and the
/// native types the program keeps by name, which a guest's host begins for it,
/// and the most bytes the engine allows a read of one piece of mail or config to
/// allocate, which hosts and their guests read within. In a guest, it says which types are the guest's own: a child of any other is
/// begun by its host (ADR-0015).
pub struct Instances {
    guests: Arc<dyn Guests>,
    links: Arc<Links>,
    own: Own,
    read_limit: usize,
    loaded: Vec<Loaded>,
    /// How many instances have been loaded into them: the number the next one loaded is handed.
    loads: u64,
}

impl Default for Instances {
    /// No instances, for an engine that runs no guests: they read nothing, as nothing is loaded into them.
    fn default() -> Self {
        Self::new(Arc::new(NoGuests), Arc::default(), 0)
    }
}

/// Which actor types the code a part's turns run begins where it runs.
#[derive(Clone, Copy)]
enum Own {
    /// Every type it spawns: a native program's (ADR-0009).
    Every,
    /// Only the types its blob makes: a guest's, whose host begins a child of any other (ADR-0015).
    #[cfg_attr(not(feature = "guest"), expect(dead_code, reason = "only a guest's library holds some types alone"))]
    Made(&'static [&'static Declared]),
}

/// An instance a host loads into a part's [`Instances`]: whatever the host keeps there, which says what memory it
/// holds for the engine's figures (ADR-0053).
pub trait Loadable: Any + Send {
    /// The memory it holds: [`Memory::Uncounted`] unless it is a guest's.
    fn memory(&self) -> Memory {
        Memory::Uncounted
    }
}

/// What memory an instance holds, as its host counts it (ADR-0053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// None the engine counts: it is not a guest's, or no actor runs in it.
    Uncounted,
    /// A guest's: the bytes of its memory, how many actors run in it, and the first of them, which names it.
    Guest { bytes: u64, actors: u32, first: Seated },
}

/// One instance a host loaded: its type, the key the host gave it, and the instance.
struct Loaded {
    host: TypeId,
    key: u64,
    instance: Held,
}

/// An instance lent out of its part's instances to a call into it, and where it was kept.
pub struct Lent<I> {
    at: usize,
    instance: Box<I>,
}

impl<I> Deref for Lent<I> {
    type Target = I;

    fn deref(&self) -> &I {
        &self.instance
    }
}

impl<I> DerefMut for Lent<I> {
    fn deref_mut(&mut self) -> &mut I {
        &mut self.instance
    }
}

/// Where a loaded instance is.
enum Held {
    Here(Box<dyn Loadable>),
    /// Lent to a call into it that is under way, which gives it back when it returns.
    Lent,
}

impl Instances {
    /// No instances yet, of the code `guests` holds, in a program that keeps the native types `links` by name, whose
    /// reads of mail and config allocate at most `read_limit` bytes.
    #[must_use]
    pub fn new(guests: Arc<dyn Guests>, links: Arc<Links>, read_limit: usize) -> Self {
        Self { guests, links, own: Own::Every, read_limit, loaded: Vec::new(), loads: 0 }
    }

    /// No instances, for a guest's turns, whose own types are `made`: a child of any other is begun by its host,
    /// through `host` (ADR-0015). Its reads allocate at most `read_limit` bytes, as its host's do.
    #[cfg(feature = "guest")]
    #[must_use]
    pub fn of_guest(host: Arc<dyn Guests>, made: &'static [&'static Declared], read_limit: usize) -> Self {
        Self { guests: host, links: Arc::default(), own: Own::Made(made), read_limit, loaded: Vec::new(), loads: 0 }
    }

    /// Whether they are a guest's.
    #[cfg(feature = "guest")]
    #[must_use]
    pub const fn is_guest(&self) -> bool {
        matches!(self.own, Own::Made(_))
    }

    /// The guests' code the instances are loaded from.
    #[must_use]
    pub fn guests(&self) -> Arc<dyn Guests> {
        Arc::clone(&self.guests)
    }

    /// The native types the program keeps by name.
    #[must_use]
    pub fn links(&self) -> Arc<Links> {
        Arc::clone(&self.links)
    }

    /// The most bytes a read of one piece of mail or config allocates, as the engine allows.
    #[must_use]
    pub const fn read_limit(&self) -> usize {
        self.read_limit
    }

    /// Whether the code the turns run begins a child of the type `declared` where it runs: any type, natively, and
    /// only its own in a guest.
    #[inline]
    #[must_use]
    pub fn is_own(&self, declared: &Declared) -> bool {
        match self.own {
            Own::Every => true,
            Own::Made(made) => made.iter().any(|own| own.id() == declared.id()),
        }
    }

    /// The instance of type `I` kept under `key`, loaded with `load` when there is none yet, handed the instance's
    /// number.
    ///
    /// # Panics
    ///
    /// Panics while the instance is lent to a call into it, which nothing reaches it through until the call gives it
    /// back.
    pub fn get_or_load<I: Loadable>(&mut self, key: u64, load: impl FnOnce(u64) -> I) -> &mut I {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        let at = found.unwrap_or_else(|| {
            let instance = Held::Here(Box::new(load(self.number())));
            self.loaded.push(Loaded { host, key, instance });
            self.loaded.len() - 1
        });
        let Held::Here(instance) = &mut self.loaded[at].instance else {
            panic!("an instance lent to a call into it is reached only once it is given back");
        };
        let instance: &mut dyn Any = &mut **instance;
        let Some(instance) = instance.downcast_mut::<I>() else {
            panic!("an instance is kept under its own type");
        };

        instance
    }

    /// The instance of type `I` kept under `key`: nothing when none is loaded, or it is lent to a call into it.
    pub fn get<I: Loadable>(&mut self, key: u64) -> Option<&mut I> {
        let host = TypeId::of::<I>();
        let loaded = self.loaded.iter_mut().find(|loaded| loaded.is(host, key))?;
        let Held::Here(instance) = &mut loaded.instance else {
            return None;
        };
        let instance: &mut dyn Any = &mut **instance;

        instance.downcast_mut::<I>()
    }

    /// Lends out the instance of type `I` kept under `key`, loaded with `load` when there is none yet, handed the
    /// instance's number, for a call into it: [`Self::put_back`] keeps it again once the call returns, and while it is
    /// out, what the call is lent reaches every other instance of the part, and none of this.
    ///
    /// # Errors
    ///
    /// [`Refused::Waiting`] while it is lent to another call: a call under way into it waits on a call out, and
    /// nothing calls into it until that returns (ADR-0015).
    ///
    /// # Panics
    ///
    /// Never: an instance is kept under its own type, so it is always read back as that type.
    pub fn lend<I: Loadable>(&mut self, key: u64, load: impl FnOnce(u64) -> I) -> Result<Lent<I>, Refused> {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        let Some(at) = found else {
            let instance = Box::new(load(self.number()));
            self.loaded.push(Loaded { host, key, instance: Held::Lent });
            return Ok(Lent { at: self.loaded.len() - 1, instance });
        };
        let Held::Here(instance) = mem::replace(&mut self.loaded[at].instance, Held::Lent) else {
            return Err(Refused::Waiting);
        };
        let instance: Box<dyn Any> = instance;
        let Ok(instance) = instance.downcast::<I>() else {
            panic!("an instance is kept under its own type");
        };

        Ok(Lent { at, instance })
    }

    /// Keeps `lent`, an instance of type `I`, under `key` again, once the call it was lent to with [`Self::lend`] has
    /// returned: where it was lent from, unless what the call was lent moved it.
    pub fn put_back<I: Loadable>(&mut self, key: u64, lent: Lent<I>) {
        let host = TypeId::of::<I>();
        let Lent { at, instance } = lent;
        let is_where = self.loaded.get(at).is_some_and(|loaded| loaded.is(host, key));
        let found = if is_where {
            Some(at)
        } else {
            self.loaded.iter().position(|loaded| loaded.is(host, key))
        };
        match found {
            Some(at) => self.loaded[at].instance = Held::Here(instance),
            None => self.loaded.push(Loaded { host, key, instance: Held::Here(instance) }),
        }
    }

    /// The number the instance loaded now takes: one no instance loaded into them before took.
    fn number(&mut self) -> u64 {
        let number = self.loads;
        self.loads += 1;

        number
    }

    /// Drops the instance of type `I` kept under `key`, for a host whose last actor that ran in it has ended: the
    /// next actor of the part that wants it loads a new one. Nothing when there is none.
    pub fn drop_instance<I: Loadable>(&mut self, key: u64) {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        if let Some(at) = found {
            self.loaded.swap_remove(at);
        }
    }
}

impl Instances {
    /// Hands `each` what memory each instance kept here holds, in the order they were loaded: for the engine's
    /// figures, between steps, when none is lent (ADR-0053).
    pub fn measure(&self, mut each: impl FnMut(Memory)) {
        for loaded in &self.loaded {
            if let Held::Here(instance) = &loaded.instance {
                each(instance.memory());
            }
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

    impl Loadable for Made {}

    impl Loadable for Other {}

    #[test]
    fn an_instance_is_loaded_once_for_its_key_and_type() {
        // Catches a lookup that misses what it loaded and loads again, a key or a host's type read as another's, and
        // a drop that takes the wrong instance or none.
        let mut instances = Instances::default();
        let mut loads = 0;
        let mut load = |_| {
            loads += 1;
            Made(loads)
        };

        let first = instances.get_or_load(1, &mut load).0;
        let again = instances.get_or_load(1, &mut load).0;
        let other_key = instances.get_or_load(2, &mut load).0;
        let other_host = instances.get_or_load(1, |_| Other(99)).0;
        instances.drop_instance::<Made>(1);
        let reloaded = instances.get_or_load(1, &mut load).0;
        let kept = instances.get_or_load(2, &mut load).0;

        assert_eq!((first, again, other_key, other_host, reloaded, kept), (1, 1, 2, 99, 3, 2));
    }

    #[test]
    fn an_instance_lent_to_a_call_is_lent_to_no_other_until_it_is_given_back() {
        // Catches a lend that loads a second instance while the first is out, one that gives out the same instance
        // twice, a lend of one key or host that takes another's out, and a give back that loses what was lent.
        let mut instances = Instances::default();
        instances.get_or_load(1, |_| Made(7));

        let lent = instances.lend(1, |_| Made(0));
        let again = instances.lend(1, |_| Made(0)).err();
        let other = instances.lend(2, |_| Made(8)).map(|made| made.0);
        let elsewhere = instances.get_or_load(1, |_| Other(9)).0;
        let reached = instances.get::<Made>(1).is_some();
        let Ok(lent) = lent else {
            panic!("an instance no call has is lent");
        };
        instances.put_back(1, lent);
        let back = instances.get::<Made>(1).map(|made| made.0);

        assert_eq!((again, other, elsewhere, reached, back), (Some(Refused::Waiting), Ok(8), 9, false, Some(7)));
    }
}
