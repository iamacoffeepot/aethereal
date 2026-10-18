//! The native guest host: guests' code run with wasmtime (ADR-0015).
//!
//! A blob's code is compiled once. Each part runs it in an instance of its
//! own, which the first actor of the blob to run in the part loads into the
//! part's [`Instances`] and every other actor of it there reuses, so nothing is
//! shared between workers and nothing is locked. A batch's items are written
//! straight into the guest's inbox, and what its turns asked for is read where
//! the guest wrote it (ADR-0015).

use aethereal_actor::records::{ItemHead, LineRecord, OutHead, SendRecord, Span, Stopped, TurnEnds};
use aethereal_actor::section::{self, Made};
use aethereal_actor::{
    Batch, Declared, FalseSend, Fault, Hosted, Instances, Item, Preparing, Refused, Role, Runs, Seated, Trapped,
    Turning,
};
use aethereal_data::{ActorId, ActorKey, Budget, PathSegment, from_bytes};
use aethereal_substrate::Engine;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::{Memory, Module, Store, TypedFunc};

/// What runs guests' code: one compiler, for every blob.
#[derive(Default)]
pub struct Guests {
    engine: wasmtime::Engine,
}

/// A blob of guest code, compiled, and what each actor type it makes declares: what every part makes its instance
/// of the blob from. Cloning it shares the compiled code.
#[derive(Clone)]
pub struct Code {
    /// What its instances are kept under in a part's [`Instances`]: a number no other code compiled in the process
    /// has.
    key: u64,
    module: Module,
    made: Vec<&'static Declared>,
}

/// The key the next code compiled takes.
static CODES: AtomicU64 = AtomicU64::new(0);

/// Why a blob of code was refused as it entered.
#[derive(Debug)]
pub enum Unfit {
    /// It is not a module the runtime compiles.
    Uncompiled(String),
    /// Its section of what it makes is missing, or does not read back.
    Undeclared,
    /// It lacks an export a guest must have, or has it with the wrong signature.
    Unexported(&'static str),
}

/// One instance of a blob, a part's, and the calls its host makes of it.
struct Instance {
    store: Store<()>,
    memory: Memory,
    inbox: TypedFunc<u32, u32>,
    out: TypedFunc<(), u64>,
    begin: TypedFunc<(u32, u64, u64, u32), u32>,
    turns: TypedFunc<(u32, u32, u32), u64>,
    /// Where the guest's inbox is, and how many bytes it has room for: asked for again only when it must grow.
    inbox_at: usize,
    inbox_room: usize,
    /// The actors that have begun in it and not ended: the part drops it when the last ends, and a trap in it ends
    /// them all.
    actors: Vec<Seated>,
}

/// An actor whose code runs in a guest's instance, as the engine holds it: the instance is its part's, which it
/// takes from what its part lends each batch and turn.
struct Guest {
    code: Code,
    me: Seated,
    /// Which actor it is in its instance.
    handle: u32,
    declared: &'static Declared,
    /// What the guest's turns of the batch prepared last asked for, and how far its host has handed them back.
    ran: Ran,
}

/// What a guest's batch left to hand back.
#[derive(Clone, Copy)]
enum Ran {
    /// No batch yet.
    Nothing,
    /// Its turns' records lie where `head` says in the guest's memory: the `next` is to be handed back, and the turns
    /// before it end where `before` says.
    At { head: OutHead, next: u32, before: TurnEnds },
    /// The call that ran them trapped, or wrote what does not read back: every turn of the batch fails.
    Trapped,
}

impl Guests {
    /// A host whose guests' compiled code is named to a profiler through a perf map: a spike's measure.
    ///
    /// # Panics
    ///
    /// Panics when the runtime cannot be made so.
    #[must_use]
    pub fn profiled() -> Self {
        let mut config = wasmtime::Config::new();
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
        let Ok(engine) = wasmtime::Engine::new(&config) else {
            panic!("a runtime that names its code to a profiler is made");
        };

        Self { engine }
    }

    /// Compiles `bytes` as guest code, and reads what it makes: refused when it is not fit to run.
    ///
    /// # Errors
    ///
    /// Why it is not fit.
    pub fn code(&self, bytes: &[u8]) -> Result<Code, Unfit> {
        let module = Module::new(&self.engine, bytes).map_err(|error| Unfit::Uncompiled(error.to_string()))?;
        let section = declared_section(bytes).ok_or(Unfit::Undeclared)?;
        let mut budget = Budget::new(1 << 20);
        let made = from_bytes::<Vec<Made>>(section, &mut budget).map_err(|_| Unfit::Undeclared)?;
        for export in ["memory", "aethereal_inbox", "aethereal_out", "aethereal_begin", "aethereal_turns"] {
            let has = module.get_export(export).is_some();
            if !has {
                return Err(Unfit::Unexported(export));
            }
        }

        let key = CODES.fetch_add(1, Ordering::Relaxed);

        Ok(Code { key, module, made: made.iter().map(Made::leak).collect() })
    }
}

impl Code {
    /// What the `of`th type it makes declares.
    #[must_use]
    pub fn declared(&self, of: usize) -> Option<&'static Declared> {
        self.made.get(of).copied()
    }

    /// Its instance in the part whose `instances` are lent: loaded there by the first of its actors to want it.
    fn instance<'i>(&self, instances: &'i mut Instances) -> &'i mut Instance {
        instances.get_or_load(self.key, || Instance::load(self))
    }
}

impl Instance {
    /// A new instance of `code`.
    ///
    /// # Panics
    ///
    /// Panics when the code cannot be instantiated, which its checks as it entered rule out.
    fn load(code: &Code) -> Self {
        let mut store = Store::new(code.module.engine(), ());
        let Ok(instance) = wasmtime::Instance::new(&mut store, &code.module, &[]) else {
            panic!("guest code that entered is instantiated");
        };
        let memory = instance.get_memory(&mut store, "memory");
        let inbox = instance.get_typed_func(&mut store, "aethereal_inbox");
        let out = instance.get_typed_func(&mut store, "aethereal_out");
        let begin = instance.get_typed_func(&mut store, "aethereal_begin");
        let turns = instance.get_typed_func(&mut store, "aethereal_turns");
        let (Some(memory), Ok(inbox), Ok(out), Ok(begin), Ok(turns)) = (memory, inbox, out, begin, turns) else {
            panic!("guest code that entered has its exports");
        };

        Self { store, memory, inbox, out, begin, turns, inbox_at: 0, inbox_room: 0, actors: Vec::new() }
    }
}

/// Makes a root of the `of`th type `code` makes, under `key`, from the bytes of its config, in its part's instance
/// of `code`: `()` for a singleton type, as a native root is keyed.
///
/// # Errors
///
/// Refused as [`Engine::add_root_made`] refuses.
///
/// # Panics
///
/// Panics when `code` makes no `of`th type, or when the type is keyed otherwise than `key` is.
pub fn add_root<K: ActorKey>(
    engine: &mut Engine,
    code: &Code,
    of: usize,
    key: &K,
    config: &[u8],
) -> Result<ActorId, Refused> {
    let Some(declared) = code.declared(of) else {
        panic!("the code makes no type {of}");
    };
    let is_keyed_so = declared.keyed() == K::KEYED;
    assert!(is_keyed_so, "the code's type {of} is keyed as {:?}", declared.keyed());
    let segment = PathSegment::new(declared.name().clone(), key);

    engine.add_root_made(&segment, declared, |me, path, table, asked, instances| {
        let mut hosted = Hosted::beginning(me, path, table, asked, declared);
        let held = code.instance(instances);
        let of = u32::try_from(of).unwrap_or(u32::MAX);
        let Some((handle, began)) = held.begin(of, me, config, &mut hosted) else {
            // A trap in a `begin` ends every actor in the instance, as one in a turn does (ADR-0015).
            hosted.trapped(held.actors.iter().copied());
            instances.drop_instance::<Instance>(code.key);
            return Err(trapped());
        };
        began?;
        held.actors.push(me);

        Ok(Box::new(Guest { code: code.clone(), me, handle, declared, ran: Ran::Nothing }))
    })
}

impl Runs for Guest {
    fn declared(&self) -> &'static Declared {
        self.declared
    }

    /// Runs every turn of the batch in the guest, in one call into its instance, and keeps what they asked for for
    /// [`Self::turn`] to hand back. The guest stops after a turn that fails or closes it, as the engine does.
    fn prepare(&mut self, preparing: Preparing<'_>) {
        let (batch, instances) = preparing.hosted();
        let held = self.code.instance(instances);
        let (items, len) = held.hand_in(batch);
        let called = held.turns.call(&mut held.store, (self.handle, items, len));
        let wrote = called.ok().and_then(|out| held.out_head(out));
        let first = TurnEnds { sends: 0, lines: 0 };
        self.ran = wrote.map_or(Ran::Trapped, |head| Ran::At { head, next: 0, before: first });
    }

    /// Hands back the next turn [`Self::prepare`] ran: what it asked for, each send checked against what the guest
    /// declares. When the call that ran it trapped, or wrote what does not read back, the turn fails, every other actor
    /// in the instance ends with it, and the part drops the instance (ADR-0015).
    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let role = match &item {
            Item::Mail(mail) if self.declared.lists(mail.kind()) => Role::Kind(mail.kind()),
            Item::Mail(_) => Role::Others,
            Item::Ended(_) => Role::Ended,
        };
        let (mut hosted, instances) = turning.hosted(self.declared, role);
        let replayed = match self.ran {
            Ran::At { head, next, before } => {
                let held = self.code.instance(instances);
                replay(held.memory.data(&held.store), &head, next, before, &mut hosted)
                    .map(|(replayed, ends)| (replayed, Ran::At { head, next: next + 1, before: ends }))
            }
            Ran::Nothing | Ran::Trapped => None,
        };
        let Some((replayed, ran)) = replayed else {
            self.ran = Ran::Trapped;
            // What the trap left in the instance's memory cannot be trusted, so nothing runs in it again.
            if let Some(held) = instances.get::<Instance>(self.code.key) {
                hosted.trapped(held.actors.iter().copied());
            }
            instances.drop_instance::<Instance>(self.code.key);
            return Err(trapped());
        };
        self.ran = ran;

        replayed
    }

    /// Ends the actor in its part's instance, and drops the instance when it was the last actor in it. An instance a
    /// trap dropped is not loaded again for it.
    fn end(self: Box<Self>, closing: aethereal_actor::Closing<'_>) -> Result<(), Fault> {
        let instances = closing.hosted();
        let Some(held) = instances.get::<Instance>(self.code.key) else {
            return Ok(());
        };
        held.actors.retain(|actor| *actor != self.me);
        let is_last = held.actors.is_empty();
        if is_last {
            instances.drop_instance::<Instance>(self.code.key);
        }

        Ok(())
    }
}

impl Instance {
    /// Runs the `begin` of the `of`th type the code makes, for `me`, from the bytes of its config, and hands what it
    /// asked for to `hosted`: the handle the guest keeps the actor under, and whether it began. Nothing when the call
    /// trapped, or wrote what does not read back.
    fn begin(
        &mut self,
        of: u32,
        me: Seated,
        config: &[u8],
        hosted: &mut Hosted<'_>,
    ) -> Option<(u32, Result<(), Fault>)> {
        let len = self.write_inbox(config);
        let handle = self.begin.call(&mut self.store, (of, me.id.bits(), me.seat.number(), len)).ok()?;
        let out = self.out.call(&mut self.store, ()).ok()?;
        let wrote = self.out_head(out).filter(|wrote| wrote.ran == 1)?;
        let (began, _) = replay(self.memory.data(&self.store), &wrote, 0, TurnEnds { sends: 0, lines: 0 }, hosted)?;

        Some((handle, began))
    }

    /// Writes the mail of `batch` straight into the guest's inbox, each item an [`ItemHead`] and then its bytes, and
    /// answers with how many items and how many bytes: as far as its first item that is not mail, which a guest does
    /// not handle yet.
    fn hand_in(&mut self, batch: &dyn Batch) -> (u32, u32) {
        let mut items = 0;
        let mut len = 0;
        let mut is_mail = true;
        batch.each(&mut |peeked| {
            let Item::Mail(mail) = peeked.item else {
                is_mail = false;
                return;
            };
            if !is_mail {
                return;
            }
            let bytes = mail.bytes();
            let head =
                ItemHead { kind: mail.kind(), source: peeked.source, chain: peeked.chain, len: count(bytes.len()) };
            let at = len;
            len += ItemHead::BYTES + bytes.len();
            self.make_room(len);
            let inbox = &mut self.memory.data_mut(&mut self.store)[self.inbox_at..];
            inbox[at..at + ItemHead::BYTES].copy_from_slice(&head.to_bytes());
            inbox[at + ItemHead::BYTES..len].copy_from_slice(bytes);
            items += 1;
        });

        (items, count(len))
    }

    /// Copies `bytes` into the guest's inbox, and answers with how many there are.
    fn write_inbox(&mut self, bytes: &[u8]) -> u32 {
        self.make_room(bytes.len());
        self.memory.data_mut(&mut self.store)[self.inbox_at..][..bytes.len()].copy_from_slice(bytes);

        count(bytes.len())
    }

    /// Makes the guest's inbox hold at least `len` bytes, keeping what it holds: at least twice its room when it grows,
    /// so a batch that writes item after item asks for room only now and then.
    fn make_room(&mut self, len: usize) {
        let short = self.inbox_room < len;
        if !short {
            return;
        }
        let room = len.max(2 * self.inbox_room).max(256);
        let Ok(at) = self.inbox.call(&mut self.store, count(room)) else {
            panic!("a guest makes room for what it is handed");
        };
        self.inbox_at = at as usize;
        self.inbox_room = room;
    }

    /// The head of what the guest's last call asked for, which `out` says where it lies: nothing when it does not read
    /// back.
    fn out_head(&self, out: u64) -> Option<OutHead> {
        let at = usize::try_from(out >> 32).ok()?;
        let len = usize::try_from(out & u64::from(u32::MAX)).ok()?;

        OutHead::read(self.memory.data(&self.store).get(at..at + len)?)
    }
}

/// Hands the `next` turn of those `head` says the guest ran into `hosted`, read from the guest's memory `data` where
/// it wrote them, the turns before it ending where `before` says: its sends, each checked against what the guest
/// declares, its lines, and how it ended. Answers with that, and where it ends: nothing when the guest ran no such
/// turn, or its records do not read back.
fn replay(
    data: &[u8],
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    hosted: &mut Hosted<'_>,
) -> Option<(Result<(), Fault>, TurnEnds)> {
    let was_run = next < head.ran;
    if !was_run {
        return None;
    }
    let ends = TurnEnds::read(record(data, head.turns_at, next, TurnEnds::BYTES)?)?;
    for index in before.sends..ends.sends {
        let send = SendRecord::read(record(data, head.sends_at, index, SendRecord::BYTES)?)?;
        let is_refused = hosted.send(send.to, send.kind, lying(data, send.bytes)?).is_err();
        if is_refused {
            return Some((Err(Fault::new(&FalseSend).unwrap_or_else(|_| trapped())), ends));
        }
    }
    for index in before.lines..ends.lines {
        let line = LineRecord::read(record(data, head.lines_at, index, LineRecord::BYTES)?)?;
        hosted.log(line.level, str::from_utf8(lying(data, line.text)?).ok()?);
    }
    let is_last = next + 1 == head.ran;
    let ended = match head.stopped {
        _ if !is_last => Ok(()),
        Stopped::No => Ok(()),
        // A `begin` cannot close its actor, so a record that says one did does not read back.
        Stopped::Closed if !hosted.may_close() => return None,
        Stopped::Closed => {
            hosted.close();
            Ok(())
        }
        Stopped::Failed { kind, bytes } => Err(Fault::of_parts(kind, lying(data, bytes)?.to_vec())),
    };

    Some((ended, ends))
}

/// The `index`th record of `len` bytes of those that begin at `at` in the guest's memory `data`.
fn record(data: &[u8], at: u32, index: u32, len: usize) -> Option<&[u8]> {
    let start = usize::try_from(at).ok()? + usize::try_from(index).ok()? * len;

    data.get(start..start + len)
}

/// The bytes `span` says lie in the guest's memory `data`.
fn lying(data: &[u8], span: Span) -> Option<&[u8]> {
    let start = usize::try_from(span.at).ok()?;

    data.get(start..start + usize::try_from(span.len).ok()?)
}

/// A count the guest's 32-bit memory holds.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The fault of a turn that trapped.
fn trapped() -> Fault {
    let Ok(fault) = Fault::new(&Trapped) else {
        panic!("a reason that holds nothing is written");
    };

    fault
}

/// The bytes of the `aethereal.declared` section of the module `bytes`, when it has one.
fn declared_section(bytes: &[u8]) -> Option<&[u8]> {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Ok(wasmparser::Payload::CustomSection(custom)) = payload
            && custom.name() == section::NAME
        {
            return Some(custom.data());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Monotonic;
    use aethereal_actor::{Actor, Beginning, Ending, Handles, Seated, ToAny, ToSender, ToWithoutMail, Turn};
    use aethereal_data::{ActorPath, Kind, KindName, PathSegment, Plain};
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The echo guest the build script built.
    const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

    /// A ping, as the echo guest declares it: the same name and layout, so the same kind.
    #[derive(Plain)]
    struct Ping {
        n: u64,
    }

    impl Kind for Ping {
        const NAME: KindName = KindName::new("aether.guest.ping");
    }

    #[derive(Plain)]
    struct Pong {
        n: u64,
    }

    impl Kind for Pong {
        const NAME: KindName = KindName::new("aether.guest.pong");
    }

    /// How many pings a pinger sends.
    #[derive(Plain)]
    struct Volley {
        pings: u64,
    }

    impl Kind for Volley {
        const NAME: KindName = KindName::new("aether.test.volley");
    }

    /// The pongs every pinger has been answered with.
    static PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that pings the echo guest, and pings it again on each pong while it has pings left.
    #[derive(aethereal_data::Bound)]
    struct Pinger;

    impl Kind for Pinger {
        const NAME: KindName = KindName::new("aether.test.pinger");
    }

    impl Actor for Pinger {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.to_any::<(Ping,)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Pinger {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            PONGS.fetch_add(1, Ordering::Relaxed);
            if pong.n > 0 {
                turn.reply(&Ping { n: pong.n - 1 })?;
            }

            Ok(())
        }
    }

    #[test]
    fn a_native_actor_and_a_guest_answer_each_other() {
        // Catches mail lost on its way into a guest or out of it, a guest's reply that does not reach its sender, and
        // a section that does not declare the guest's handler, which its host would refuse the mail for.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64);
        let Ok(_) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(_) = engine.add_root::<Pinger>(&(), Volley { pings: 9 }) else {
            panic!("the pinger begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        assert_eq!(PONGS.load(Ordering::Relaxed), 10);
    }

    /// The word to close, as the echo guest declares it.
    #[derive(Plain)]
    struct Shut;

    impl Kind for Shut {
        const NAME: KindName = KindName::new("aether.guest.shut");
    }

    /// The pongs the closer has been answered with.
    static CLOSER_PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that sends the echo guest a ping, the word to close, and another ping, all in its `begin`.
    #[derive(aethereal_data::Bound)]
    struct Closer;

    impl Kind for Closer {
        const NAME: KindName = KindName::new("aether.test.closer");
    }

    impl Actor for Closer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: 0 })?;
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Shut)?;
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: 0 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Closer {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            CLOSER_PONGS.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }
    }

    #[test]
    fn a_guest_that_closes_answers_nothing_after() {
        // Catches a guest's close lost on its way out of the guest: the echo would then be asked for the turn of the
        // ping after it, which the guest did not run, and fail.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(_) = engine.add_root::<Closer>(&(), Volley { pings: 0 }) else {
            panic!("the closer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_substrate::Entry::Failed(_)));
        assert!(!failed, "the echo closed, and did not fail");
        assert_eq!(CLOSER_PONGS.load(Ordering::Relaxed), 1);
    }

    /// Which callers have been answered, a bit for each, and how many answers there were.
    static CALLERS_ANSWERED: AtomicU64 = AtomicU64::new(0);
    static CALLER_ANSWERS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that pings the echo guest once as it begins, and notes its answer under its key.
    #[derive(aethereal_data::Bound)]
    struct Caller {
        key: u64,
    }

    impl Kind for Caller {
        const NAME: KindName = KindName::new("aether.test.caller");
    }

    impl Actor for Caller {
        type Config = Volley;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.to_any::<(Ping,)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
            }

            Ok(Self { key: volley.pings })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Caller {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            let is_own = pong.n == self.key;
            if is_own {
                CALLERS_ANSWERED.fetch_or(1 << self.key, Ordering::Relaxed);
            }
            CALLER_ANSWERS.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }
    }

    #[test]
    fn each_of_a_batch_of_callers_is_answered_once() {
        // Catches a turn handed back what another turn of its batch sent, a send handed back twice or not at all, and
        // an item written over another as the inbox grows while a batch is written into it.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(_) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let callers = 50;
        for key in 0..callers {
            let Ok(_) = engine.add_root::<Caller>(&key, Volley { pings: key }) else {
                panic!("a caller begins");
            };
        }
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        assert_eq!(CALLERS_ANSWERED.load(Ordering::Relaxed), (1 << callers) - 1);
        assert_eq!(CALLER_ANSWERS.load(Ordering::Relaxed), callers);
    }

    /// The pongs the volleyer has been answered with.
    static VOLLEYER_PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that tells the mirror guest to close, and volleys with the echo guest, both of one blob.
    #[derive(aethereal_data::Bound)]
    struct Volleyer;

    impl Kind for Volleyer {
        const NAME: KindName = KindName::new("aether.test.volleyer");
    }

    impl Actor for Volleyer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(mirror) = turn.resolve(&mirror) {
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(mirror)).send(&Shut)?;
            }
            if let Some(echo) = turn.resolve(&echo) {
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Volleyer {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            VOLLEYER_PONGS.fetch_add(1, Ordering::Relaxed);
            if pong.n > 0 {
                turn.reply(&Ping { n: pong.n - 1 })?;
            }

            Ok(())
        }
    }

    #[test]
    fn a_guest_answers_from_its_instance_after_another_in_it_has_ended() {
        // Catches two actors of one blob in one part each loading an instance of their own, and an instance dropped
        // when an actor in it ends while another still runs in it: the echo, asked for its turns from an instance that
        // does not hold it, would fail.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(_) = add_root(&mut engine, &code, 1, &(), &[]) else {
            panic!("the mirror begins");
        };
        let Ok(_) = engine.add_root::<Volleyer>(&(), Volley { pings: 3 }) else {
            panic!("the volleyer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_substrate::Entry::Failed(_)));
        assert!(!failed, "the echo ran on in its instance");
        assert_eq!(VOLLEYER_PONGS.load(Ordering::Relaxed), 4);
    }

    /// The word to crash, as the echo guest declares it.
    #[derive(Plain)]
    struct Crash;

    impl Kind for Crash {
        const NAME: KindName = KindName::new("aether.guest.crash");
    }

    /// A native actor that tells the mirror guest to crash as it begins.
    #[derive(aethereal_data::Bound)]
    struct Crasher;

    impl Kind for Crasher {
        const NAME: KindName = KindName::new("aether.test.crasher");
    }

    impl Actor for Crasher {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Crash,), Crash>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            if let Some(mirror) = turn.resolve(&mirror) {
                turn.to_any::<(Crash,)>(ToWithoutMail::Held(mirror)).send(&Crash)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Crasher {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_trap_ends_every_actor_in_its_instance() {
        // Catches a trap that ends only the actor whose turn trapped, leaving an actor that lives in the same memory
        // looking live, and the ending a host reports for it dropped with the rest of the failed turn.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(mirror) = add_root(&mut engine, &code, 1, &(), &[]) else {
            panic!("the mirror begins");
        };
        let Ok(_) = engine.add_root::<Crasher>(&(), Volley { pings: 0 }) else {
            panic!("the crasher begins");
        };
        let (Some(echo_seat), Some(mirror_seat)) = (engine.table().seat(echo), engine.table().seat(mirror)) else {
            panic!("the echo and the mirror are seated");
        };
        let echo = Seated { id: echo, seat: echo_seat };
        let mirror = Seated { id: mirror, seat: mirror_seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let trapped = |actor| {
            let is_trap = |entry: &aethereal_substrate::Entry| match entry {
                aethereal_substrate::Entry::Failed(fault) => fault.kind() == aethereal_data::KindId::of::<Trapped>(),
                _ => false,
            };
            engine.log(actor).any(is_trap)
        };
        assert!(trapped(mirror), "the mirror's turn trapped");
        assert!(trapped(echo), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }

    #[test]
    fn a_trap_in_a_begin_ends_every_actor_in_its_instance() {
        // Catches a `begin` that traps leaving the actors already in its instance looking live, whether its host
        // reports no ending for them or the engine drops what it reported with the rest of the failed `begin`.
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };

        let doomed = add_root(&mut engine, &code, 3, &(), &[]);
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let is_trap = |entry: &aethereal_substrate::Entry| match entry {
            aethereal_substrate::Entry::Failed(fault) => fault.kind() == aethereal_data::KindId::of::<Trapped>(),
            _ => false,
        };
        let refused_trapped =
            matches!(&doomed, Err(Refused::Began(fault)) if fault.kind() == aethereal_data::KindId::of::<Trapped>());
        assert!(refused_trapped, "the doomed root is refused with its trap");
        assert!(engine.log(echo).any(is_trap), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }
}
