//! The instances a part's actors run their code in, which the engine carries with the part and never looks into
//! (ADR-0015).

use crate::{Declared, Guests, Links, NoGuests, Refused};
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
/// A host keys what it loads by a number of its own, under the type it loads, so no host reaches another's.
///
/// It also carries the engine's [`Guests`], the code it loads from, so a turn
/// that spawns a child from a blob reaches it through what it is lent, and the
/// native types the program keeps by name, which a guest's host begins for it.
/// In a guest, it says which types are the guest's own: a child of any other is
/// begun by its host (ADR-0015).
pub struct Instances {
    guests: Arc<dyn Guests>,
    links: Arc<Links>,
    own: Own,
    loaded: Vec<Loaded>,
}

impl Default for Instances {
    /// No instances, for an engine that runs no guests.
    fn default() -> Self {
        Self::new(Arc::new(NoGuests), Arc::default())
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
    Here(Box<dyn Any + Send>),
    /// Lent to a call into it that is under way, which gives it back when it returns.
    Lent,
}

impl Instances {
    /// No instances yet, of the code `guests` holds, in a program that keeps the native types `links` by name.
    #[must_use]
    pub fn new(guests: Arc<dyn Guests>, links: Arc<Links>) -> Self {
        Self { guests, links, own: Own::Every, loaded: Vec::new() }
    }

    /// No instances, for a guest's turns, whose own types are `made`: a child of any other is begun by its host,
    /// through `host` (ADR-0015).
    #[cfg(feature = "guest")]
    #[must_use]
    pub fn of_guest(host: Arc<dyn Guests>, made: &'static [&'static Declared]) -> Self {
        Self { guests: host, links: Arc::default(), own: Own::Made(made), loaded: Vec::new() }
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

    /// The instance of type `I` kept under `key`, loaded with `load` when there is none yet.
    ///
    /// # Panics
    ///
    /// Panics while the instance is lent to a call into it, which nothing reaches it through until the call gives it
    /// back.
    pub fn get_or_load<I: Any + Send>(&mut self, key: u64, load: impl FnOnce() -> I) -> &mut I {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        let at = found.unwrap_or_else(|| {
            self.loaded.push(Loaded { host, key, instance: Held::Here(Box::new(load())) });
            self.loaded.len() - 1
        });
        let Held::Here(instance) = &mut self.loaded[at].instance else {
            panic!("an instance lent to a call into it is reached only once it is given back");
        };
        let Some(instance) = instance.downcast_mut::<I>() else {
            panic!("an instance is kept under its own type");
        };

        instance
    }

    /// The instance of type `I` kept under `key`: nothing when none is loaded, or it is lent to a call into it.
    pub fn get<I: Any + Send>(&mut self, key: u64) -> Option<&mut I> {
        let host = TypeId::of::<I>();
        let loaded = self.loaded.iter_mut().find(|loaded| loaded.is(host, key))?;
        let Held::Here(instance) = &mut loaded.instance else {
            return None;
        };

        instance.downcast_mut::<I>()
    }

    /// Lends out the instance of type `I` kept under `key`, loaded with `load` when there is none yet, for a call into
    /// it: [`Self::put_back`] keeps it again once the call returns, and while it is out, what the call is lent reaches
    /// every other instance of the part, and none of this.
    ///
    /// # Errors
    ///
    /// [`Refused::Waiting`] while it is lent to another call: a call under way into it waits on a call out, and
    /// nothing calls into it until that returns (ADR-0015).
    ///
    /// # Panics
    ///
    /// Never: an instance is kept under its own type, so it is always read back as that type.
    pub fn lend<I: Any + Send>(&mut self, key: u64, load: impl FnOnce() -> I) -> Result<Lent<I>, Refused> {
        let host = TypeId::of::<I>();
        let found = self.loaded.iter().position(|loaded| loaded.is(host, key));
        let Some(at) = found else {
            self.loaded.push(Loaded { host, key, instance: Held::Lent });
            return Ok(Lent { at: self.loaded.len() - 1, instance: Box::new(load()) });
        };
        let Held::Here(instance) = mem::replace(&mut self.loaded[at].instance, Held::Lent) else {
            return Err(Refused::Waiting);
        };
        let Ok(instance) = instance.downcast::<I>() else {
            panic!("an instance is kept under its own type");
        };

        Ok(Lent { at, instance })
    }

    /// Keeps `lent`, an instance of type `I`, under `key` again, once the call it was lent to with [`Self::lend`] has
    /// returned: where it was lent from, unless what the call was lent moved it.
    pub fn put_back<I: Any + Send>(&mut self, key: u64, lent: Lent<I>) {
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

    #[test]
    fn an_instance_lent_to_a_call_is_lent_to_no_other_until_it_is_given_back() {
        // Catches a lend that loads a second instance while the first is out, one that gives out the same instance
        // twice, a lend of one key or host that takes another's out, and a give back that loses what was lent.
        let mut instances = Instances::default();
        instances.get_or_load(1, || Made(7));

        let lent = instances.lend(1, || Made(0));
        let again = instances.lend(1, || Made(0)).err();
        let other = instances.lend(2, || Made(8)).map(|made| made.0);
        let elsewhere = instances.get_or_load(1, || Other(9)).0;
        let reached = instances.get::<Made>(1).is_some();
        let Ok(lent) = lent else {
            panic!("an instance no call has is lent");
        };
        instances.put_back(1, lent);
        let back = instances.get::<Made>(1).map(|made| made.0);

        assert_eq!((again, other, elsewhere, reached, back), (Some(Refused::Waiting), Ok(8), 9, false, Some(7)));
    }
}
