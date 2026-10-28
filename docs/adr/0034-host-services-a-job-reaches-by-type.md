# ADR-0034: Host services a job reaches, by type

- **Status:** Proposed
- **Date:** 2026-10-28

## Context

A job sometimes needs something from its host that its own code cannot do on both hosts. A file read is `std::fs` natively and a `fetch` in a browser. A dial out is a blocking connect natively, and a browser cannot make one at all. A job's code is linked unchanged into both builds (ADR-0018: `JobType`, `JobCode::Native`), so the host-specific part has to come from the host, through something the running job is handed.

ADR-0018 handed every running job one such thing: the host's timer (`Running::until`, `Timer`). That is the engine's own. The engine ships `Sleep`, which every host runs, so the timer is part of what a job is, and it belongs in `aethereal-actor`.

ADR-0032 needed a second one, the host's storage, and gave it the timer's whole route: "the host's storage reaches a job as its timer does". `Storage`, `Namespace`, `Unread`, and `NoStorage` went into `aethereal-actor`. `Running` gained a field and `Running::storage`. `JobType::drive` gained an argument, and so did every runner (`Inline`, the native `Runners` and their `Io` thread, the browser's `EventLoop`). Every host's serving gained a field, a method, and a `Serves<FileHosting>` impl. The timer was the only precedent, and its route was taken whole without asking whether storage was the engine's own. It is not: one capability, `Files`, uses it.

ADR-0033's dial was about to take the same route a third time (`Dialer`, `Running::dialer`). The cost is plain by then. Each capability with a host service touches five layers in every host: a trait in the actor crate, a field on `Running`, an argument to `drive`, a field in every runner, and a field and an impl in every host's serving. Adding the dialer changed fourteen files. The core crates would come to name every capability that needs its host, which is the opposite of building the engine from pieces.

## Decision

**What a job reaches of its host is of two kinds, and only the engine's own is named in the core.**

- **The engine's own** is what every host must give every job because the engine's own jobs need it: today, the timer, for `Sleep`. It stays as ADR-0018 has it: `Timer` in `aethereal-actor`, `Running::until`, and an argument to `JobType::drive`.
- **A capability's own** is what one capability's jobs need of the host: the storage files are read from, the dialer connections are made through, and whatever comes next. The core does not name it. The capability's crate defines it, the hosts that can implement it, and the core carries it without knowing what it is.

**A capability's service is a trait in the capability's crate, and a job finds it on `Running` by its type.**

```rust
// aethereal-actor: holds services, names none
pub struct Services { .. }                                   // a set of host services, at most one of each type
impl Services {
    pub fn none() -> Self;
    pub fn with<S: ?Sized + Send + Sync + 'static>(self, service: Arc<S>) -> Self;
}
impl Running {
    pub fn service<S: ?Sized + 'static>(&self) -> Option<&S>;   // the host's service of type S, when it serves one
}

// aethereal-files: the capability owns its trait
pub trait Storage: Send + Sync { fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath) -> Reading<'a>; }

impl Job for Load {
    async fn run(self, job: &mut Running) -> Result<FileRead, Fault> {
        let Some(storage) = job.service::<dyn Storage>() else { return Ok(FileRead::Unavailable) };
        // ...
    }
}
```

- **A service is keyed by its type,** usually a trait object (`dyn Storage`), and held as an `Arc` of it. The set holds at most one of each type. Serving a second one of a type replaces the first, the same as a host whose second root serves the same service.
- **`Running::service` is a lookup that can miss.** A host that serves no storage leaves it out of the set, and the job answers as its capability decides: `Load` answers `FileRead::Unavailable`, as `NoStorage` did. **This departs from a stand-in service for each absence** (`NoStorage`, `NoDialer`): an absent service is absent, and the capability that looks for it says what that means.
- **The set is fixed once the runners are made.** ADR-0029 has a host make its runners once its roots have bootstrapped, with what they served. The set is built then, wrapped in an `Arc`, and never changes, so it takes no lock.

**A runner carries the set and nothing else of its host's services.** `JobType::drive` takes the timer and the set: `drive(bytes, lent, limit, timer, services)`. Every runner holds one `Arc<Services>` and hands it to every job it drives: `Inline`, the native runners and their `Io` thread, and the browser's event loop. A capability added later changes none of them.

**A capability's bootstrap serves its service into the set, through one generic end.**

```rust
// aethereal-actor
pub struct Service<S: ?Sized> { .. }                         // one service on its way into a host's set
impl<S: ?Sized + Send + Sync + 'static> Service<S> { pub fn of(service: Arc<S>) -> Self; }

// each host's crate, once: every service of every capability
impl<S: ?Sized + Send + Sync + 'static> Serves<Service<S>> for Headless { .. }

// a capability's bootstrap on a host
cx.serve(Service::<dyn Storage>::of(Arc::new(Directories::new(&roots))));
```

- **A host implements `Serves<Service<S>>` once, for every `S`.** Its serving keeps a `Services`, and adds each service a root serves. **This departs from a hosting type and a `Serves` impl for each capability** (`FileHosting`, `Serves<FileHosting>`, `Serving::files`), which are removed.
- **The ends a host drives itself keep their own `Serves` impls:** the window's `Hosting` and the renderer's `Drawing` (ADR-0024, ADR-0021). These are not services a job reaches. They are fronts the host's event loop works through, and a host that cannot drive one should fail to build when a root serves it.
- **A host's crate still implements each service it can,** and writes each capability's bootstrap on it (ADR-0029). That is where host-specific code belongs: `Directories` in the native host's crate, and the browser's fetching in the browser's. Only the core's plumbing stops naming capabilities.

**What moves:**

- `Storage`, `Reading`, `Unread`, and `Namespace` move from `aethereal-actor` to `aethereal-files`. `NoStorage`, `Running::storage`, and `FileHosting` are removed.
- ADR-0032's statement "the host's storage reaches a job as its timer does", and the `Storage` sketch beside it, are replaced by this record.
- ADR-0033's dialer becomes `aethereal-tcp`'s: `Dialer`, `Dialing`, and `Undialed` live in that crate, `Dial` finds the dialer with `job.service::<dyn Dialer>()`, and a host with none is answered `Reached::Unavailable`. `Running::dialer` and `NoDialer` are never added.

## Consequences

- A capability that needs its host adds a trait to its own crate, an implementation and a bootstrap to each host that can serve it, and nothing to `aethereal-actor`, `aethereal-substrate`, or any runner.
- `aethereal-actor` names one host service, the timer. What it holds beyond that, it holds without knowing what it is.
- A job that looks for a service its host does not serve finds none, and its capability answers as it decides. The type system does not stop a program from listing a capability whose service its host cannot give. That check stays where ADR-0029 puts it: a host that cannot serve a capability does not write that capability's bootstrap, so a program that lists it on that host does not build.
- A service's type is its trait object. Two capabilities that defined the same trait would be one service, which cannot happen, since each trait belongs to one crate.
- Finding a service is a lookup in a small map by type id, once each time a job asks. Jobs ask once per run, so the cost does not matter.
- `JobType::drive` and every runner lose the storage argument and gain the set, so the next capability changes none of them.

## Alternatives considered

- **The timer's route for every service** (ADR-0032 as first written). Each capability with a host service touches five layers in every host, and the core comes to name every capability.
- **One struct of every host service, `HostServices { timer, storage, dialer, .. }`.** One argument instead of several, but still a type in the core that names every capability, and edited for each new one.
- **A service generic on the job, `Job::Service`, handed to `run`.** The runner would have to know each job's service type to find it, which is the naming this record removes.
- **A service as a resource an actor owns** (ADR-0010), passed in the job. A job is given no resource (ADR-0018), and a service is shared by every job that runs at once, not owned by one actor.
- **A stand-in for each absent service** (`NoStorage`, `NoDialer`). Each capability would ship one, and every host would have to serve every capability's stand-in. An absent entry in the set says the same thing.
- **Keying by a name instead of a type.** Two capabilities could pick the same name, and the lookup would need a cast that might fail. A trait object's type id is unique and needs no cast.
