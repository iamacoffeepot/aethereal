//! The engine: it runs turns in steps, and applies what they asked for (ADR-0009, ADR-0011).

mod begun;
mod since;
#[cfg(test)]
mod tests;

use crate::id::{child_id, root_id};
use crate::{ActorTable, Envelope, Standing, Tie};
use aethereal_actor::{
    Actor, Asked, Beginning, Code, Effect, Ending, Mail, MailId, Refused, Runs, Stood, To, Turn, What, Why,
};
use aethereal_data::{ActorId, ActorPath, KindId, PathSegment};
use begun::begun;
use since::Since;
use std::collections::BTreeMap;
use std::iter;
use std::mem;
use std::num::NonZeroUsize;

/// The engine: its actors, the table of them, and the steps their turns run in (ADR-0011).
///
/// A step runs every actor that has mail, one turn for each piece it had when
/// the step began. When the turns have finished the engine folds their
/// effects into the table, then delivers the mail they sent. Mail sent in a
/// step is handled in a later one.
///
/// A step here runs its turns one after another, on the thread that calls
/// it. That is the whole of a browser host's engine, and the order in which
/// a native host's workers run the same turns does not change what they
/// leave.
pub struct Engine {
    /// Which actors there are: the first view.
    table: ActorTable,
    /// Every actor the engine still runs, under its id: the live ones, and the ended ones whose closing turn is
    /// still to run.
    seats: BTreeMap<u64, Seat>,
    /// The actors themselves, each under its id. An actor leaves when its closing turn takes it.
    actors: BTreeMap<u64, Box<dyn Runs>>,
    /// The number of the last step run. No step has run at zero.
    step: u64,
    /// The most bytes one read of a piece of mail may allocate.
    read_limit: usize,
}

/// What the engine keeps for one actor beside the actor itself.
struct Seat {
    /// Where the actor stands.
    path: ActorPath,
    /// The mail delivered to it and not yet handled, in the order it will be.
    queue: Vec<Envelope>,
    /// How many pieces of mail it has sent.
    sent: u64,
    /// Whether it still takes mail.
    life: Life,
}

/// Whether an actor the engine runs still takes mail.
#[derive(Clone, Copy)]
enum Life {
    /// It handles its mail.
    Live,
    /// It has ended, and its closing turn runs in the next step.
    Ending { why: Why, chain: Chain },
}

/// The chain a turn's mail belongs to (ADR-0012).
#[derive(Clone, Copy)]
struct Chain {
    /// The mail whose turn sends. None for a root's `begin`, which nothing caused: fixed when the chain is made.
    cause: Option<MailId>,
    /// The first mail of the chain.
    root: MailId,
}

/// One finished turn, as the engine applies it.
struct Ran {
    /// What the turn asked for, in order.
    effects: Vec<Effect>,
    /// The children it stood up.
    stood: Vec<Stood>,
    /// The chain its mail belongs to.
    chain: Chain,
    /// The sender of the mail it handled. None for a `begin` and a closing turn, which handle none: fixed when
    /// the turn is run.
    sender: Option<ActorId>,
}

impl Engine {
    /// An engine with no actors, its table in `parts` parts, whose turns read at most `read_limit` bytes into
    /// any one piece of mail.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize) -> Self {
        Self { table: ActorTable::new(parts), seats: BTreeMap::new(), actors: BTreeMap::new(), step: 0, read_limit }
    }

    /// The table of actors, as of the last step.
    #[must_use]
    pub const fn table(&self) -> &ActorTable {
        &self.table
    }

    /// The number of the last step run, from one. Zero before the first.
    #[must_use]
    pub const fn step_number(&self) -> u64 {
        self.step
    }

    /// Whether a step would run no turn: no actor has mail, and none has a closing turn to run.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.seats.values().all(Seat::is_idle)
    }

    /// Makes a root of type `A` under `key`, for the chassis, and answers with its id (ADR-0008).
    ///
    /// The root's `begin` runs here, and what it asked for is applied before this returns, as a step's effects
    /// are. A root is made between steps.
    ///
    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds for another path (ADR-0008).
    pub fn add_root<A: Actor + 'static>(&mut self, key: &A::Key, config: A::Config) -> Result<ActorId, Refused> {
        let segment = PathSegment::new(A::NAME, key);
        let id = root_id(&segment);
        let path = ActorPath::from_root(&segment);
        let is_taken = self.table.path(id).as_ref() == Some(&path);
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        let mut asked = Asked::new();
        let root =
            A::begin(config, &mut Beginning::new(id, path.clone(), &self.table, &mut asked)).map_err(Refused::Began)?;
        let (asked, stood) = asked.into_parts();
        let from = Code::Linked(A::NAME);
        let made = match segment.key() {
            None => What::Root { from },
            Some(key) => What::RootInstanced { from, key: key.clone() },
        };
        let effects = iter::once(Effect::new(id, made)).chain(asked).collect();
        let chain = Chain { cause: None, root: MailId { sender: id, count: 0 } };

        self.seat(id, path, Box::new(root));
        self.apply(vec![Ran { effects, stood, chain, sender: None }], &[]);

        Ok(id)
    }

    /// Runs one step: a turn for each piece of mail each actor had when the step began, then what they asked
    /// for (ADR-0011).
    ///
    /// # Panics
    ///
    /// Panics when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn step(&mut self) {
        self.step += 1;

        let mut ran = Vec::new();
        let mut gone = Vec::new();
        let ids: Vec<ActorId> = self.seats.keys().map(|bits| ActorId::new(*bits)).collect();
        for id in ids {
            self.run(id, &mut ran, &mut gone);
        }

        self.apply(ran, &gone);
    }

    /// Runs the turns of the actor `id` stands for: its closing turn when it has ended, and otherwise one turn
    /// for each piece of mail in its queue.
    ///
    /// An actor that closes or fails handles no more mail, so what was queued behind that turn is dropped.
    fn run(&mut self, id: ActorId, ran: &mut Vec<Ran>, gone: &mut Vec<ActorId>) {
        let Some(seat) = self.seats.get_mut(&id.bits()) else {
            return;
        };

        match seat.life {
            Life::Ending { why, chain } => {
                let Some(actor) = self.actors.remove(&id.bits()) else {
                    return;
                };
                let mut asked = Asked::new();
                let ended = actor.end(&mut Ending::new(id, seat.path.clone(), &self.table, &mut asked, why));
                // A closing turn that fails asks for nothing: only its log lines would be kept (ADR-0009).
                let effects = match ended {
                    Ok(()) => asked.into_parts().0,
                    Err(_) => Vec::new(),
                };

                ran.push(Ran { effects, stood: Vec::new(), chain, sender: None });
                gone.push(id);
            }
            Life::Live => {
                let Some(actor) = self.actors.get_mut(&id.bits()) else {
                    return;
                };
                let mut stood_since = Vec::new();
                for envelope in mem::take(&mut seat.queue) {
                    let table = Since { table: &self.table, stood: &stood_since };
                    let mut asked = Asked::new();
                    let mail = Mail::new(envelope.kind, &envelope.bytes, envelope.id.sender, &table, self.read_limit);
                    let handled = actor.handle(mail, &mut Turn::new(id, seat.path.clone(), &table, &mut asked));
                    // A turn that fails changes nothing outside its actor: what it asked for is dropped, and the
                    // engine writes the one effect that says the actor ended (ADR-0009).
                    let (effects, stood) = match handled {
                        Ok(()) => asked.into_parts(),
                        Err(_) => (vec![Effect::new(id, What::Failed)], Vec::new()),
                    };
                    let chain = Chain { cause: Some(envelope.id), root: envelope.root };
                    let ends = effects.iter().any(|effect| ending(effect, id).is_some());

                    stood_since.extend(stood.iter().map(|child| (child.id(), child.path().clone())));
                    ran.push(Ran { effects, stood, chain, sender: Some(envelope.id.sender) });
                    if ends {
                        break;
                    }
                }
            }
        }
    }

    /// Applies finished turns, in the order given: folds, then delivers (ADR-0011).
    ///
    /// Every turn's effects are folded into the table and its children made live before any mail is delivered,
    /// so a child spawned in a step is there for mail sent to it in that step, and mail to an actor that ended
    /// in the step is not delivered. The actors in `gone` have run their closing turn, and leave last, once
    /// their last mail is out.
    fn apply(&mut self, ran: Vec<Ran>, gone: &[ActorId]) {
        let mut sent = Vec::new();
        for Ran { effects, stood, chain, sender } in ran {
            let effects = begun(effects);
            self.table.fold(&effects);

            for child in stood {
                self.seat(child.id(), child.path().clone(), child.into_actor());
            }
            for effect in &effects {
                if let Some(why) = ending(effect, effect.by()) {
                    self.end(effect.by(), why, chain);
                }
            }

            sent.push((effects, chain, sender));
        }

        for (effects, chain, sender) in sent {
            for effect in effects {
                let by = effect.by();
                if let What::Send { to, kind, bytes } = effect.into_what() {
                    self.deliver(by, &to, kind, bytes, chain, sender);
                }
            }
        }

        for seat in self.seats.values_mut() {
            seat.queue.sort_by_key(|envelope| (envelope.id.sender.bits(), envelope.id.count));
        }
        for id in gone {
            self.seats.remove(&id.bits());
        }
    }

    /// Gives the actor `id` stands for, at `path`, a seat: it is run from the next step.
    fn seat(&mut self, id: ActorId, path: ActorPath, actor: Box<dyn Runs>) {
        self.seats.insert(id.bits(), Seat { path, queue: Vec::new(), sent: 0, life: Life::Live });
        self.actors.insert(id.bits(), actor);
    }

    /// Marks the actor `id` stands for as ended because of `why`: its closing turn runs in the next step, in
    /// the chain of the mail that brought its ending about (ADR-0012).
    fn end(&mut self, id: ActorId, why: Why, chain: Chain) {
        if let Some(seat) = self.seats.get_mut(&id.bits()) {
            seat.life = Life::Ending { why, chain };
        }
    }

    /// Makes the mail `by` sent to `to` and puts it in its receiver's queue.
    ///
    /// The mail takes the next of its sender's counts whether or not it is delivered. It is not delivered when
    /// `to` names no actor, or one that is not live: mail that is owed nothing ends there (ADR-0012).
    fn deliver(&mut self, by: ActorId, to: &To, kind: KindId, bytes: Vec<u8>, chain: Chain, sender: Option<ActorId>) {
        let Some(from) = self.seats.get_mut(&by.bits()) else {
            return;
        };
        from.sent += 1;
        let id = MailId { sender: by, count: from.sent };

        let receiver = match to {
            To::Parent => self.table.parent(by),
            To::Child(kind) => Some(child_id(by, &PathSegment::new(kind.clone(), &()))),
            To::ChildInstanced(kind, key) => Some(child_id(by, &PathSegment::instanced(kind.clone(), key.clone()))),
            To::Sender => sender,
            To::Held(reference) => Some(reference.id()),
        };
        let Some(receiver) = receiver.filter(|to| self.table.standing(*to) == Standing::Live) else {
            return;
        };
        let Some(seat) = self.seats.get_mut(&receiver.bits()) else {
            return;
        };

        seat.queue.push(Envelope {
            id,
            to: receiver,
            cause: chain.cause,
            root: chain.root,
            tie: Tie::Tell,
            kind,
            bytes,
        });
    }
}

impl Seat {
    /// Whether a step would run no turn of this actor.
    fn is_idle(&self) -> bool {
        let has_mail = !self.queue.is_empty();
        let is_ending = matches!(self.life, Life::Ending { .. });

        !(has_mail || is_ending)
    }
}

/// Why the actor `id` stands for ends, when `effect` is that actor's own ending.
fn ending(effect: &Effect, id: ActorId) -> Option<Why> {
    let is_own = effect.by() == id;
    match effect.what() {
        What::Close if is_own => Some(Why::Closed),
        What::Failed if is_own => Some(Why::Faulted),
        _ => None,
    }
}
