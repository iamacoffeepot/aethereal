//! The engine: it runs turns in steps, and applies what they asked for (ADR-0009, ADR-0011).

mod begun;
mod since;
#[cfg(test)]
mod tests;

use crate::id::root_id;
use crate::stream::{Pending, Streams};
use crate::watch::Watchers;
use crate::{ActorTable, Context, Folded};
use aethereal_actor::{
    Actor, Asked, Beginning, Code, Effect, Ending, Fault, How, Item, Mail, MailId, Refused, Runs, Stood, Turn, What,
    Why,
};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use begun::begun;
use since::Since;
use std::collections::BTreeMap;
use std::iter;
use std::num::NonZeroUsize;

/// The engine: its actors, the views of them, and the steps their turns run in (ADR-0011).
///
/// A step runs every actor that has something to react to, one turn for each
/// item in its stream as the step began. When the turns have finished the
/// engine folds their effects into the views, each with the context it was
/// emitted in. What a step's turns sent is in the streams for a later step.
///
/// A step here runs its turns one after another, on the thread that calls
/// it. That is the whole of a browser host's engine, and the order in which
/// a native host's workers run the same turns does not change what they
/// leave.
pub struct Engine {
    /// Which actors there are: the first view.
    table: ActorTable,
    /// Who watches each actor.
    watchers: Folded<Watchers>,
    /// What each actor has to react to.
    streams: Folded<Streams>,
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
    /// How many pieces of mail it has sent, as of the last step applied.
    sent: u64,
    /// Whether it still reacts.
    life: Life,
}

/// Whether an actor the engine runs still reacts.
enum Life {
    /// It reacts to its stream.
    Live,
    /// It has ended, `how` it ended, and its closing turn runs in the next step, in `context`.
    Ending { how: How, context: Context },
}

/// One finished turn, as the engine applies it.
struct Ran {
    /// What the turn asked for, in order.
    effects: Vec<Effect>,
    /// The children it stood up.
    stood: Vec<Stood>,
    /// The context its effects were emitted in.
    context: Context,
}

impl Engine {
    /// An engine with no actors, its views in `parts` parts, whose turns read at most `read_limit` bytes into
    /// any one piece of mail.
    #[must_use]
    pub fn new(parts: NonZeroUsize, read_limit: usize) -> Self {
        Self {
            table: ActorTable::new(parts),
            watchers: Folded::new(parts),
            streams: Folded::new(parts),
            seats: BTreeMap::new(),
            actors: BTreeMap::new(),
            step: 0,
            read_limit,
        }
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

    /// Whether a step would run no turn: no actor has anything to react to, and none has a closing turn to run.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.seats.iter().all(|(bits, seat)| {
            let has_items = self.has_items(*bits);
            let is_ending = matches!(seat.life, Life::Ending { .. });

            !(has_items || is_ending)
        })
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
        let context = Context { cause: None, root: MailId { sender: id, count: 0 } };

        self.seat(id, path, Box::new(root));
        self.apply(vec![Ran { effects, stood, context }], &[]);

        Ok(id)
    }

    /// Runs one step: a turn for each item each actor had to react to when the step began, then what they asked
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
    /// for each item in its stream.
    ///
    /// An actor that closes or fails handles nothing more, so what was in its stream behind that turn is dropped.
    fn run(&mut self, id: ActorId, ran: &mut Vec<Ran>, gone: &mut Vec<ActorId>) {
        let items = self.take_items(id);
        let Some(seat) = self.seats.get_mut(&id.bits()) else {
            return;
        };

        match &seat.life {
            Life::Ending { how, context } => {
                let Some(actor) = self.actors.remove(&id.bits()) else {
                    return;
                };
                let mut asked = Asked::new();
                let why = why(how);
                let ended = actor.end(&mut Ending::new(id, seat.path.clone(), &self.table, &mut asked, seat.sent, why));
                // A closing turn that fails asks for nothing: only its log lines would be kept (ADR-0009). Either
                // way the engine adds that the actor's life is over, after what the turn sent.
                let mut effects = match ended {
                    Ok(()) => asked.into_parts().0,
                    Err(_) => Vec::new(),
                };
                effects.push(Effect::new(id, What::Ended(how.clone())));

                ran.push(Ran { effects, stood: Vec::new(), context: *context });
                gone.push(id);
            }
            Life::Live => {
                let Some(actor) = self.actors.get_mut(&id.bits()) else {
                    return;
                };
                let mut stood_since = Vec::new();
                let mut count = seat.sent;
                for pending in items {
                    let table = Since { table: &self.table, stood: &stood_since };
                    let mut asked = Asked::new();
                    let source = pending.source();
                    let (item, context) = match &pending {
                        Pending::Mail(envelope) => {
                            let mail = Mail::new(envelope.kind, &envelope.bytes, source, &table, self.read_limit);

                            (Item::Mail(mail), Context { cause: Some(envelope.id), root: envelope.root })
                        }
                        Pending::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
                    };
                    let mut turn = Turn::new(id, seat.path.clone(), &table, &mut asked, count, source);
                    let handled = actor.handle(item, &mut turn);
                    let turned = Ran::of(id, handled, asked, context);

                    count = turned.sent(id).unwrap_or(count);
                    stood_since.extend(turned.stood.iter().map(|child| (child.id(), child.path().clone())));
                    let over = turned.ends(id);
                    ran.push(turned);
                    if over {
                        break;
                    }
                }
            }
        }
    }

    /// Applies finished turns, in the order given (ADR-0011).
    ///
    /// Every turn's effects are folded into the table and the watchers, and its children made live, before any is
    /// folded into the streams, which read both: so a child spawned in a step is there for mail sent to it in that
    /// step, and mail to an actor that ended in the step is not kept. The actors in `gone` have run their closing
    /// turn, and leave last.
    fn apply(&mut self, ran: Vec<Ran>, gone: &[ActorId]) {
        let ran: Vec<Ran> = ran.into_iter().map(|ran| Ran { effects: begun(ran.effects), ..ran }).collect();

        let mut folded = Vec::with_capacity(ran.len());
        for Ran { effects, stood, context } in ran {
            self.table.fold(&effects, &context);
            self.watchers.fold(&effects, &context, ());

            for child in stood {
                self.seat(child.id(), child.path().clone(), child.into_actor());
            }
            for effect in &effects {
                self.count(effect);
                if let Some(how) = ending(effect, effect.by()) {
                    self.end(effect.by(), how, context);
                }
            }

            folded.push((effects, context));
        }
        for (effects, context) in &folded {
            self.streams.fold(effects, context, (&self.table, &self.watchers));
        }

        for id in gone {
            self.seats.remove(&id.bits());
            self.take_items(*id);
        }
    }

    /// Gives the actor `id` stands for, at `path`, a seat: it is run from the next step.
    fn seat(&mut self, id: ActorId, path: ActorPath, actor: Box<dyn Runs>) {
        self.seats.insert(id.bits(), Seat { path, sent: 0, life: Life::Live });
        self.actors.insert(id.bits(), actor);
    }

    /// Marks the actor `id` stands for as ended, `how` it ended: its closing turn runs in the next step, in the
    /// chain of the mail that brought its ending about (ADR-0012).
    fn end(&mut self, id: ActorId, how: How, context: Context) {
        if let Some(seat) = self.seats.get_mut(&id.bits()) {
            seat.life = Life::Ending { how, context };
        }
    }

    /// Keeps the count of a send applied: the next turn of its sender counts on from it.
    fn count(&mut self, effect: &Effect) {
        let What::Send { id, .. } = effect.what() else {
            return;
        };
        if let Some(seat) = self.seats.get_mut(&id.sender.bits()) {
            seat.sent = seat.sent.max(id.count);
        }
    }

    /// Whether the actor whose bits these are has anything in its stream.
    fn has_items(&self, bits: u64) -> bool {
        self.streams.shard(bits).get(&bits).is_some_and(|items| !items.is_empty())
    }

    /// Takes everything in the stream of the actor `id` stands for, in the order it is to be handed it.
    fn take_items(&mut self, id: ActorId) -> Vec<Pending> {
        let bits = id.bits();
        let taken = self.streams.shard_mut(bits).remove(&bits);

        taken.into_iter().flat_map(BTreeMap::into_values).collect()
    }
}

impl Ran {
    /// The turn of the actor `id` stands for that returned `outcome`, having asked for `asked`, in `context`.
    ///
    /// A turn that fails changes nothing outside its actor: what it asked for is dropped, and the engine writes
    /// the one effect that says the actor ended (ADR-0009).
    fn of(id: ActorId, outcome: Result<(), Fault>, asked: Asked, context: Context) -> Self {
        let (effects, stood) = match outcome {
            Ok(()) => asked.into_parts(),
            Err(fault) => (vec![Effect::new(id, What::Failed(fault))], Vec::new()),
        };

        Self { effects, stood, context }
    }

    /// Whether the turn ends the actor `id` stands for.
    fn ends(&self, id: ActorId) -> bool {
        self.effects.iter().any(|effect| ending(effect, id).is_some())
    }

    /// The count of the last mail the actor `id` stands for sent in this turn, or none when it sent none.
    fn sent(&self, id: ActorId) -> Option<u64> {
        self.effects
            .iter()
            .filter_map(|effect| match effect.what() {
                What::Send { id: mail, .. } if mail.sender == id => Some(mail.count),
                _ => None,
            })
            .max()
    }
}

/// How the actor `id` stands for ends, when `effect` is that actor's own ending.
fn ending(effect: &Effect, id: ActorId) -> Option<How> {
    let is_own = effect.by() == id;
    match effect.what() {
        What::Close if is_own => Some(How::Closed),
        What::Failed(fault) if is_own => Some(How::Failed(fault.clone())),
        _ => None,
    }
}

/// Why a closing turn runs, for an actor that ended `how`.
const fn why(how: &How) -> Why {
    match how {
        How::Failed(_) => Why::Faulted,
        How::Closed | How::Earlier => Why::Closed,
    }
}
