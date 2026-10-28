//! Jobs: work that takes time, run by the host outside the steps, bytes in and bytes out (ADR-0018).
//!
//! A turn runs a job with [`Turn::run`](crate::Turn::run), keeping a context,
//! and is answered in a later step, as it is for a request it asked
//! (ADR-0017): the answer comes from the actor to itself, tied to the job, and
//! is handed to its handler of [`Answer<J::Done, C>`](crate::Answer) with
//! the context.

#[cfg(feature = "std")]
use crate::blob::handed_ticket;
use crate::{
    Actors, ActorsReader, ActorsWriter, BlobId, Fault, Kept, MailId, Seated, Storage, Time, Timer, Until, Unwritable,
    Within,
};
#[cfg(feature = "std")]
use crate::{Dialer, Given, Resource, pins};
use aethereal_data::{
    ActorId, Budget, Decode, DecodeError, Encode, EncodeError, Field, Kind, KindId, Reader, Schema, Shape, Writer,
};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;
use core::future::{self, Future};
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Work the host runs outside the steps: bytes in, bytes out (ADR-0018). It touches no engine state while it runs and
/// is given no resource, so it runs wherever its host can run it. It may answer with a resource it made.
#[allow(async_fn_in_trait, reason = "a job's future is made and driven where it runs, and never moves between threads")]
pub trait Job: Kind + Encode<Actors> + Decode<Actors> + 'static {
    /// What it answers with.
    type Done: Kind + Encode<Actors> + Decode<Actors> + 'static;

    /// Where it runs.
    const RUNNER: Runner;

    /// Runs it, driven by its host, never by the engine: a computation's future is ready the first time it is
    /// polled.
    ///
    /// # Errors
    ///
    /// The fault its runner answers the actor with, as [`Unanswered::Faulted`](crate::Unanswered::Faulted).
    async fn run(self, job: &mut Running) -> Result<Self::Done, Fault>;
}

/// Where a job runs (ADR-0018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    /// On threads for computing.
    Compute,
    /// On a reactor for waiting, so a job that waits on a socket holds no thread a computation could use.
    Io,
}

/// What a running job reads and makes beside its own bytes: a copy of each blob it refers to, the blobs it puts, and
/// the resources it hands (ADR-0018).
pub struct Running {
    blobs: Vec<Copied>,
    /// The engine that ran it, which the blobs it puts are numbered for.
    engine: u32,
    /// The bytes of each blob it has put, in the order put.
    put: Vec<Vec<u8>>,
    /// Each resource it has handed, in the order handed, under the ticket the job gave it.
    handed: Vec<Kept>,
    /// Its host's timer.
    timer: Arc<dyn Timer>,
    /// Its host's storage.
    storage: Arc<dyn Storage>,
    /// Its host's dialer.
    #[cfg(feature = "std")]
    dialer: Arc<dyn Dialer>,
}

#[cfg(feature = "std")]
impl Running {
    /// The bytes of the blob `blob` refers to: a copy made as the job was handed out, or one the job put. Every
    /// reference a job holds was read out of its bytes or made by its puts, so a reference to any other, which no job
    /// can come to hold, reads as no bytes.
    #[must_use]
    pub fn read(&self, blob: &crate::BlobRef) -> &[u8] {
        let id = blob.id();
        if let Some(index) = id.made_index() {
            let put = usize::try_from(index).ok().and_then(|index| self.put.get(index));
            return put.map_or(&[], |bytes| &bytes[..]);
        }
        let copied = self.blobs.iter().find(|copied| copied.id == id);

        copied.map_or(&[], |copied| &copied.bytes)
    }

    /// Waits until `at` on the host's timer, the clock its steps' times are read from: for a job that waits for time,
    /// as [`Sleep`](crate::Sleep) does, or backs off before it tries again.
    pub fn until(&self, at: Time) -> Until<'_> {
        Until::new(at, &*self.timer)
    }

    /// The host's storage, which a job reads files from (ADR-0032).
    #[must_use]
    pub fn storage(&self) -> &dyn Storage {
        &*self.storage
    }

    /// The host's dialer, which a job connects to another machine through (ADR-0033).
    #[must_use]
    pub fn dialer(&self) -> &dyn Dialer {
        &*self.dialer
    }

    /// Puts `bytes` as a blob, which nobody changes from then on, and answers with a reference to it, to hold in what
    /// the job answers with. The engine puts it in its store when the job is answered, out of its budget for blobs,
    /// and writes the blob's own reference over this one where it lies in the answer; a blob that does not fit
    /// answers the job [`Faulted`](crate::Unanswered::Faulted). So no reference a job made reaches an actor before its
    /// blob is in the store.
    pub fn put(&mut self, bytes: Vec<u8>) -> crate::BlobRef {
        let index = u64::try_from(self.put.len()).unwrap_or(u64::MAX);
        self.put.push(bytes);

        crate::BlobRef::pin(BlobId::made(self.engine, index))
    }

    /// Hands `value`, a resource the job made, and answers with it given, to hold in what the job answers with. The
    /// engine keeps it when the job is answered, and writes its own ticket over this one where it lies in the answer,
    /// so the actor answered takes it in the handler of the answer, as it takes a resource in any mail (ADR-0010). One
    /// the answer does not hold is dropped with the job, as is every one when the job is cancelled or faults.
    pub fn hand<R: Resource>(&mut self, value: R) -> Given<R> {
        let index = u64::try_from(self.handed.len()).unwrap_or(u64::MAX);
        let ticket = handed_ticket(index);
        self.handed.push(Kept::new(ticket, value));

        Given::new(ticket)
    }
}

/// What a job a host ran made: its result's bytes, written at [`Actors`], a pin on each blob it refers to, at where
/// its reference begins in them, the bytes of each blob it put, which the references among them to those blobs name by
/// the order they were put in, and each resource it handed that its result holds, at where its ticket lies (ADR-0018).
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct Produced {
    /// Its result's bytes.
    pub bytes: Vec<u8>,
    /// A pin on each blob its result refers to, kept in the order of where they lie.
    pub held: Vec<crate::Pinning>,
    /// The bytes of each blob it put, in the order put.
    pub put: Vec<Vec<u8>>,
    /// Each resource it handed that its result holds, with where its ticket lies in the result's bytes, in the order
    /// handed.
    pub handed: Vec<(u32, Kept)>,
}

#[cfg(feature = "std")]
impl Produced {
    /// `done`, a job's result, which put no blob: written at [`Actors`] with the pins on the blobs it refers to.
    ///
    /// # Errors
    ///
    /// Why it could not be written: it holds a resource.
    pub fn of<K: Encode<Actors>>(done: &K) -> Result<Self, EncodeError> {
        let mut held = Vec::new();
        let mut out = Writer::<Actors>::appending(Vec::new(), ActorsWriter::new(&mut held));
        done.encode(&mut out)?;

        Ok(Self { bytes: out.into_bytes(), held, put: Vec::new(), handed: Vec::new() })
    }

    /// `done`, a job's result, written at [`Actors`] with the pins on the blobs it refers to, and with each of
    /// `handed`, what the job handed, that it holds. A resource handed and not held is dropped.
    ///
    /// # Errors
    ///
    /// Why it could not be written: it holds a resource the job did not hand, or one twice.
    fn handing<K: Encode<Actors>>(done: &K, handed: Vec<Kept>) -> Result<Self, EncodeError> {
        let mut held = Vec::new();
        let mut tickets = Vec::new();
        let writer = ActorsWriter::handed(&mut held, handed.len(), &mut tickets);
        let mut out = Writer::<Actors>::appending(Vec::new(), writer);
        done.encode(&mut out)?;
        let bytes = out.into_bytes();
        let written_at = |kept: &Kept| tickets.iter().find(|&&(_, ticket)| ticket == kept.ticket()).map(|&(at, _)| at);
        let handed = handed.into_iter().filter_map(|kept| written_at(&kept).map(|at| (at, kept))).collect();

        Ok(Self { bytes, held, put: Vec::new(), handed })
    }
}

/// What a job is lent beside its bytes: a copy of each blob it refers to, and where the changes to their pins it
/// makes as it runs go (ADR-0018).
#[cfg(feature = "std")]
pub struct Lending {
    /// A copy of each blob it refers to.
    pub blobs: Vec<Copied>,
    /// The engine that ran it.
    engine: u32,
    /// Where the changes to that engine's pins go while it runs.
    queue: pins::Queue,
}

#[cfg(feature = "std")]
impl Lending {
    /// The copies `blobs`, lent by the engine numbered `engine`, the changes to whose pins go onto `queue`.
    #[must_use]
    pub const fn new(engine: u32, blobs: Vec<Copied>, queue: pins::Queue) -> Self {
        Self { blobs, engine, queue }
    }
}

/// A blob a job refers to, as its host is handed it: which it is, and a copy of its bytes, made as the job is handed
/// out (ADR-0018).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// The blob.
    pub id: BlobId,
    /// A copy of its bytes.
    pub bytes: Vec<u8>,
}

/// A job a turn ran: its mail's id, the actor that ran it and the count it took among what it sends (ADR-0018).
///
/// It means something only to the actor that ran it, as a watch's id does. It crosses [`Within`] and no other boundary,
/// so it is kept in the actor's state and its contexts, and never in mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(MailId);

impl Submitted {
    /// The job's id.
    #[must_use]
    pub const fn id(&self) -> JobId {
        JobId(MailId { sender: self.by.id, count: self.count })
    }
}

impl JobId {
    /// The job of `id`.
    #[must_use]
    pub const fn new(id: MailId) -> Self {
        Self(id)
    }

    /// Its mail's id.
    #[must_use]
    pub const fn mail(self) -> MailId {
        self.0
    }
}

/// A job's id is its mail's id in bytes.
impl Schema for JobId {
    const SHAPE: Shape = Shape::Struct {
        name: "JobId",
        fields: &[Field { name: "sender", shape: u64::SHAPE }, Field { name: "count", shape: u64::SHAPE }],
    };
}

impl Encode<Within> for JobId {
    fn encode(&self, out: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
        let MailId { sender, count } = self.0;
        sender.bits().encode(out)?;
        count.encode(out)
    }
}

impl Decode<Within> for JobId {
    fn decode(input: &mut Reader<'_, Within>) -> Result<Self, DecodeError> {
        let sender = ActorId::new(u64::decode(input)?);
        let count = u64::decode(input)?;

        Ok(Self(MailId { sender, count }))
    }
}

/// Why a turn ran no job (ADR-0018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrun {
    /// The job could not be written: it holds a resource, or a collection too long to count.
    Unwritten(EncodeError),
}

impl From<Unrun> for Fault {
    fn from(unrun: Unrun) -> Self {
        match unrun {
            Unrun::Unwritten(_) => Self::bare::<Unwritable>(),
        }
    }
}

/// A job a turn ran, as the engine takes it out to its host once the turn has run (ADR-0018).
#[derive(Debug)]
pub struct Submitted {
    /// The actor that ran it.
    pub by: Seated,
    /// The count it took among what the actor sends.
    pub count: u64,
    /// The code that runs it.
    pub code: JobCode,
    /// The kind it is answered with.
    pub answer: KindId,
    /// Its bytes, written at [`Actors`].
    pub bytes: Vec<u8>,
    /// Each blob it refers to, as often as it refers to it: kept by the context the actor keeps until it is answered.
    pub refers: Vec<BlobId>,
}

/// The code that runs a job a turn ran (ADR-0018).
#[derive(Debug, Clone, Copy)]
pub enum JobCode {
    /// A native turn's: the code of the type it ran, which it names as it runs it, so its host runs it with no lookup
    /// and runs every job a native turn runs.
    Native(JobType),
    /// A guest's: the kind of the job alone, as a guest cannot hand its host code. The engine runs it with the code of
    /// the job type of that kind the program lists for its guests, and answers it that no runner runs it when the
    /// program lists none (ADR-0015).
    Guest(KindId),
}

impl JobCode {
    /// The kind of the job.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        match self {
            Self::Native(of) => of.kind(),
            Self::Guest(kind) => *kind,
        }
    }
}

/// What a job a host runs comes to: what it made, or its fault.
#[cfg(feature = "std")]
pub type Outcome = Result<Produced, Fault>;

/// A job's future, as its host drives it.
pub type Driven = Pin<Box<dyn Future<Output = Outcome>>>;

/// How a host runs a job of one type: from its bytes, what it is lent, the most bytes it is read within, the host's
/// timer, its storage, and its dialer, to the future its host drives.
#[cfg(feature = "std")]
type Drive = fn(Vec<u8>, Lending, usize, Arc<dyn Timer>, Arc<dyn Storage>, Arc<dyn Dialer>) -> Driven;

/// A job type, as a host runs it, whatever the type: what a native turn hands its host with each job it runs, and what
/// a program lists of each job type its guests may run (ADR-0015, ADR-0018).
#[cfg(feature = "std")]
#[derive(Clone, Copy)]
pub struct JobType {
    kind: KindId,
    runner: Runner,
    drive: Drive,
}

/// A job type shows its kind and where it runs: its code is a function's address.
#[cfg(feature = "std")]
impl fmt::Debug for JobType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JobType")
            .field("kind", &self.kind)
            .field("runner", &self.runner)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "std")]
impl JobType {
    /// The job type `J`.
    #[must_use]
    pub fn of<J: Job>() -> Self {
        Self { kind: KindId::of::<J>(), runner: J::RUNNER, drive: drive::<J> }
    }

    /// The kind of its jobs.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// Where its jobs run.
    #[must_use]
    pub const fn runner(&self) -> Runner {
        self.runner
    }

    /// The future that runs the job of `bytes`, which is lent `lent`, read within `limit` bytes, waits on `timer`,
    /// reads files from `storage`, and connects through `dialer`, where it is driven: so the job's value is read on the
    /// thread or worker that runs it, and never moves (ADR-0018, ADR-0032, ADR-0033).
    #[must_use]
    pub fn drive(
        &self,
        bytes: Vec<u8>,
        lent: Lending,
        limit: usize,
        timer: Arc<dyn Timer>,
        storage: Arc<dyn Storage>,
        dialer: Arc<dyn Dialer>,
    ) -> Driven {
        (self.drive)(bytes, lent, limit, timer, storage, dialer)
    }
}

/// The job types a program's guests may run, listed as a tuple, `(Fetch, Decode)`: the code the engine runs a guest's
/// job with, as a guest cannot hand its host code (ADR-0015, ADR-0029).
#[cfg(feature = "std")]
pub trait JobList {
    /// Each type listed, in order.
    fn types() -> Vec<JobType>;
}

#[cfg(feature = "std")]
impl JobList for () {
    fn types() -> Vec<JobType> {
        Vec::new()
    }
}

#[cfg(feature = "std")]
macro_rules! job_lists {
    ($( [$($job:ident)*] )*) => {$(
        impl<$($job: Job),*> JobList for ($($job,)*) {
            fn types() -> Vec<JobType> {
                alloc::vec![$(JobType::of::<$job>()),*]
            }
        }
    )*};
}

#[cfg(feature = "std")]
job_lists! {
    [A]
    [A B]
    [A B C]
    [A B C D]
    [A B C D E]
    [A B C D E F]
    [A B C D E F G]
    [A B C D E F G H]
    [A B C D E F G H I]
    [A B C D E F G H I J]
    [A B C D E F G H I J K]
    [A B C D E F G H I J K L]
}

/// The future that reads the job `J` of `bytes`, which is lent `lent`, within `limit` bytes, runs it on `timer`,
/// `storage`, and `dialer`, and
/// writes what it answers with. Each reference read out of the job is a pin of its own, and each change to a pin made as it is polled
/// or dropped goes onto the queue it was lent.
#[cfg(feature = "std")]
fn drive<J: Job>(
    bytes: Vec<u8>,
    lent: Lending,
    limit: usize,
    timer: Arc<dyn Timer>,
    storage: Arc<dyn Storage>,
    dialer: Arc<dyn Dialer>,
) -> Driven {
    let Lending { blobs, engine, queue } = lent;
    let running = Box::pin(async move {
        let mut budget = Budget::new(limit);
        let pins = crate::PinSource::job(&blobs);
        let job = J::decode(&mut Reader::<Actors>::new(&bytes, &mut budget, ActorsReader::of(&pins)))?;
        let mut running = Running { blobs, engine, put: Vec::new(), handed: Vec::new(), timer, storage, dialer };
        let done = job.run(&mut running).await?;
        let Running { put, handed, .. } = running;
        let produced = Produced::handing(&done, handed)?;

        Ok(Produced { put, ..produced })
    });

    Box::pin(Queued { running, engine, queue })
}

/// A job's future, each poll and its drop run with the changes to its engine's pins going onto that engine's queue, so
/// a host's thread that runs it takes no lock to change one (ADR-0018).
#[cfg(feature = "std")]
struct Queued {
    running: Driven,
    engine: u32,
    queue: pins::Queue,
}

#[cfg(feature = "std")]
impl Future for Queued {
    type Output = Outcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Outcome> {
        let Self { running, engine, queue } = &mut *self;

        pins::queued(*engine, queue, || running.as_mut().poll(context))
    }
}

/// A job dropped before it finished, cancelled or lost, drops what it held as it would have as it ran.
#[cfg(feature = "std")]
impl Drop for Queued {
    fn drop(&mut self) {
        let running = mem::replace(&mut self.running, Box::pin(future::pending()));
        pins::queued(self.engine, &self.queue, || drop(running));
    }
}

/// `value`, written at [`Actors`] into bytes of its own, and a pin on each blob it refers to, as often as it refers to
/// it: refused when it holds a resource.
///
/// # Errors
///
/// Why it could not be written.
#[cfg(feature = "std")]
pub fn lending<K: Encode<Actors>>(value: &K) -> Result<(Vec<u8>, Vec<crate::BlobRef>), EncodeError> {
    let mut held = Vec::new();
    let mut out = Writer::<Actors>::appending(Vec::new(), ActorsWriter::new(&mut held));
    value.encode(&mut out)?;
    let bytes = out.into_bytes();

    Ok((bytes, held.into_iter().map(|(_, pin)| pin).collect()))
}
