//! The host's services a running job reaches by type: each defined by the capability whose jobs need it, and carried
//! here without being named (ADR-0034).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::any::{Any, TypeId};

/// The services a host serves its jobs, at most one of each type: each an `Arc` of the capability's own trait object,
/// such as `dyn Storage` (ADR-0034). Built once, as the host makes its runners, and never changed after. Copying it
/// copies only the `Arc`s it holds.
#[derive(Default, Clone)]
pub struct Services {
    by_type: BTreeMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Services {
    /// None served.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// These and `service`, which replaces one of its type served before.
    #[must_use]
    pub fn with<S: ?Sized + Send + Sync + 'static>(mut self, service: Arc<S>) -> Self {
        self.by_type.insert(TypeId::of::<S>(), Arc::new(service));

        self
    }

    /// The service of type `S`, when one is served.
    #[must_use]
    pub fn get<S: ?Sized + 'static>(&self) -> Option<&S> {
        let served = self.by_type.get(&TypeId::of::<S>())?;

        served.downcast_ref::<Arc<S>>().map(|service| &**service)
    }
}

/// One service on its way into a host's set, as a capability's bootstrap serves it (ADR-0029: `cx.serve`). A host
/// serves every service the same way, whatever its type, so it names none (ADR-0034).
pub struct Service<S: ?Sized> {
    service: Arc<S>,
}

impl<S: ?Sized + Send + Sync + 'static> Service<S> {
    /// `service`, to serve as the service of type `S`.
    #[must_use]
    pub const fn of(service: Arc<S>) -> Self {
        Self { service }
    }

    /// `services` with this one added.
    #[must_use]
    pub fn added_to(self, services: Services) -> Services {
        services.with(self.service)
    }
}
