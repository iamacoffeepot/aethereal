//! The native guest host: guests' code run with wasmtime (ADR-0015).
//!
//! A spike: one instance for each blob, shared by all its actors behind a
//! lock, and one item handed in for each call.

use aethereal_actor::section::{self, Made};
use aethereal_actor::{
    Declared, FalseSend, Fault, Hosted, Item, Level, Refused, Role, Runs, SeatNumber, Seated, Trapped, Turning,
};
use aethereal_data::{ActorId, Budget, KindId, PathSegment, from_bytes};
use aethereal_substrate::Engine;
use std::sync::{Arc, Mutex, MutexGuard};
use wasmtime::{Memory, Module, Store, TypedFunc};

/// What runs guests' code: one compiler, for every blob.
pub struct Guests {
    engine: wasmtime::Engine,
}

/// A blob of guest code, compiled, and what each actor type it makes declares.
pub struct Code {
    module: Module,
    made: Vec<&'static Declared>,
}

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

/// One instance of a blob, and the calls its host makes of it.
pub struct Instance {
    store: Store<()>,
    memory: Memory,
    inbox: TypedFunc<u32, u32>,
    out: TypedFunc<(), u64>,
    begin: TypedFunc<(u32, u64, u64, u32), u32>,
    turn: TypedFunc<(u32, u32, u64, u64, u64, u64, u32), u64>,
    /// Where the guest's inbox is, and how many bytes it has room for: asked for again only when it must grow.
    inbox_at: usize,
    inbox_room: usize,
}

/// An instance its actors share.
pub type Shared = Arc<Mutex<Instance>>;

/// An actor whose code runs in a guest's instance, as the engine holds it.
struct Guest {
    instance: Shared,
    handle: u32,
    declared: &'static Declared,
}

impl Default for Guests {
    fn default() -> Self {
        Self { engine: wasmtime::Engine::default() }
    }
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
        for export in ["memory", "aethereal_inbox", "aethereal_out", "aethereal_begin", "aethereal_turn"] {
            let has = module.get_export(export).is_some();
            if !has {
                return Err(Unfit::Unexported(export));
            }
        }

        Ok(Code { module, made: made.iter().map(Made::leak).collect() })
    }

    /// A new instance of `code`.
    ///
    /// # Panics
    ///
    /// Panics when the code cannot be instantiated, which its checks as it entered rule out.
    #[must_use]
    pub fn instance(&self, code: &Code) -> Shared {
        let mut store = Store::new(&self.engine, ());
        let Ok(instance) = wasmtime::Instance::new(&mut store, &code.module, &[]) else {
            panic!("guest code that entered is instantiated");
        };
        let memory = instance.get_memory(&mut store, "memory");
        let inbox = instance.get_typed_func(&mut store, "aethereal_inbox");
        let out = instance.get_typed_func(&mut store, "aethereal_out");
        let begin = instance.get_typed_func(&mut store, "aethereal_begin");
        let turn = instance.get_typed_func(&mut store, "aethereal_turn");
        let (Some(memory), Ok(inbox), Ok(out), Ok(begin), Ok(turn)) = (memory, inbox, out, begin, turn) else {
            panic!("guest code that entered has its exports");
        };

        Arc::new(Mutex::new(Instance { store, memory, inbox, out, begin, turn, inbox_at: 0, inbox_room: 0 }))
    }
}

impl Code {
    /// What the `of`th type it makes declares.
    #[must_use]
    pub fn declared(&self, of: usize) -> Option<&'static Declared> {
        self.made.get(of).copied()
    }
}

/// Makes a root of the `of`th type `code` makes, in `instance`, from the bytes of its config.
///
/// # Errors
///
/// Refused as [`Engine::add_root_made`] refuses.
///
/// # Panics
///
/// Panics when `code` makes no `of`th type.
pub fn add_root(
    engine: &mut Engine,
    code: &Code,
    instance: &Shared,
    of: usize,
    config: &[u8],
) -> Result<ActorId, Refused> {
    let Some(declared) = code.declared(of) else {
        panic!("the code makes no type {of}");
    };
    let segment = PathSegment::new(declared.name().clone(), &());

    engine.add_root_made(segment, declared, |me, path, table, asked| {
        let mut hosted = Hosted::beginning(me, path, table, asked, declared);
        let mut guard = lock(instance);
        let held = &mut *guard;
        let len = held.write_inbox(&[config]);
        let of = u32::try_from(of).unwrap_or(u32::MAX);
        let handle = held.begin.call(&mut held.store, (of, me.id.bits(), me.seat.number(), len));
        let Ok(handle) = handle else {
            return Err(trapped());
        };
        let Ok(out) = held.out.call(&mut held.store, ()) else {
            return Err(trapped());
        };
        held.replay(out, &mut hosted)?;
        drop(guard);

        Ok(Box::new(Guest { instance: Arc::clone(instance), handle, declared }))
    })
}

impl Runs for Guest {
    fn declared(&self) -> &'static Declared {
        self.declared
    }

    fn handle(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let Item::Mail(mail) = item else {
            return Ok(());
        };
        let kind = mail.kind();
        let role = if self.declared.lists(kind) {
            Role::Kind(kind)
        } else {
            Role::Others
        };
        let mut hosted = turning.hosted(self.declared, role);
        let source = mail.sender_seated();
        let chain = hosted.chain();
        let mut guard = lock(&self.instance);
        let held = &mut *guard;
        let len = held.write_inbox(&[&kind.plain_bytes(), mail.bytes()]);
        let args = (
            self.handle,
            len,
            source.id.bits(),
            source.seat.number(),
            chain.root.sender.bits(),
            chain.root.count,
            chain.depth,
        );
        let Ok(out) = held.turn.call(&mut held.store, args) else {
            return Err(trapped());
        };

        held.replay(out, &mut hosted)
    }

    fn end(self: Box<Self>, _: aethereal_actor::Closing<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Instance {
    /// Copies `parts`, one after another, into the guest's inbox, and answers with how many bytes they are.
    fn write_inbox(&mut self, parts: &[&[u8]]) -> u32 {
        let len: usize = parts.iter().map(|part| part.len()).sum();
        let short = self.inbox_room < len;
        if short {
            let room = len.max(256);
            let Ok(at) = self.inbox.call(&mut self.store, u32::try_from(room).unwrap_or(u32::MAX)) else {
                panic!("a guest makes room for what it is handed");
            };
            self.inbox_at = at as usize;
            self.inbox_room = room;
        }
        let mut at = self.inbox_at;
        let data = self.memory.data_mut(&mut self.store);
        for part in parts {
            data[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }

        u32::try_from(len).unwrap_or(u32::MAX)
    }

    /// Hands what the guest's last call asked for, which lies at `out`, to `hosted`, reading it where it lies in the
    /// guest's memory: its
    /// mail, each send checked, and its lines; and answers with its fault when it failed.
    fn replay(&self, out: u64, hosted: &mut Hosted<'_>) -> Result<(), Fault> {
        let at = (out >> 32) as usize;
        let len = (out & u64::from(u32::MAX)) as usize;
        let data = self.memory.data(&self.store);
        let Some(mut out) = data.get(at..at + len) else {
            return Err(trapped());
        };
        let reading = read_out(&mut out, hosted);

        reading.unwrap_or_else(|| Err(trapped()))
    }
}

/// Reads what a guest's call asked for out of `out` into `hosted`: nothing when `out` does not read back.
fn read_out(out: &mut &[u8], hosted: &mut Hosted<'_>) -> Option<Result<(), Fault>> {
    let failed = take_u32(out)? == 1;
    let fault = if failed {
        let kind = take_kind(out)?;
        let len = take_u32(out)? as usize;
        Some(Fault::of_parts(kind, take(out, len)?.to_vec()))
    } else {
        None
    };
    let mailed = take_u32(out)?;
    for _ in 0..mailed {
        let id = take_u64(out)?;
        let seat = take_u64(out)?;
        let kind = take_kind(out)?;
        let len = take_u32(out)? as usize;
        let bytes = take(out, len)?;
        let to = Seated { id: ActorId::new(id), seat: SeatNumber::new(seat) };
        let sent = hosted.send(to, kind, bytes);
        if sent.is_err() {
            return Some(Err(Fault::new(&FalseSend).unwrap_or_else(|_| trapped())));
        }
    }
    let lines = take_u32(out)?;
    for _ in 0..lines {
        let level = match take_u32(out)? {
            0 => Level::Debug,
            1 => Level::Info,
            2 => Level::Warn,
            _ => Level::Error,
        };
        let len = take_u32(out)? as usize;
        let text = std::str::from_utf8(take(out, len)?).ok()?;
        hosted.log(level, text);
    }

    Some(fault.map_or(Ok(()), Err))
}

fn take<'a>(out: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (taken, rest) = out.split_at_checked(len)?;
    *out = rest;

    Some(taken)
}

fn take_u32(out: &mut &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(take(out, 4)?.try_into().ok()?))
}

fn take_u64(out: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(out, 8)?.try_into().ok()?))
}

fn take_kind(out: &mut &[u8]) -> Option<KindId> {
    let mut budget = Budget::new(8);

    from_bytes::<KindId>(take(out, 8)?, &mut budget).ok()
}

/// The fault of a turn that trapped.
fn trapped() -> Fault {
    let Ok(fault) = Fault::new(&Trapped) else {
        panic!("a reason that holds nothing is written");
    };

    fault
}

/// The instance `shared` holds, for the one worker that runs its part.
fn lock(shared: &Shared) -> MutexGuard<'_, Instance> {
    match shared.lock() {
        Ok(held) => held,
        Err(poisoned) => poisoned.into_inner(),
    }
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
    use aethereal_actor::{Actor, Beginning, Ending, Handles, ToAny, ToSender, ToWithoutMail, Turn};
    use aethereal_data::{ActorPath, Kind, KindName, Plain};
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
        let instance = guests.instance(&code);
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64);
        let Ok(_) = add_root(&mut engine, &code, &instance, 0, &[]) else {
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
}
