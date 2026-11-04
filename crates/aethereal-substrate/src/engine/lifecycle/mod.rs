//! The program's lifecycle, which the engine runs between steps: each stage sent to the actors that subscribe to it,
//! and the next sent only once every turn the last set off has run (ADR-0020).
//!
//! A stage reaches each subscriber as mail from that actor to itself, as what
//! a source brings in does (ADR-0010), and every piece of one stage's mail has
//! one root, so a stage is one chain, however far its mail goes. Whether it
//! has finished is read between steps from what the engine still holds of it
//! (`Engine::has_finished`).

#[cfg(test)]
mod tests;

use super::{Engine, ROOTS, parity};
use crate::id::root_id;
use crate::lives;
use crate::stages::{self, key};
// A request's deadline, named apart from the lifecycle's own `Due`, which says when a cycle is.
use aethereal_actor::Due as RequestDue;
use aethereal_actor::{Cycle, Cycled, Cycling, Declared, MailId, Mailed, Refused, Seated, Start, Stop, Tie, Time};
use aethereal_data::{ActorId, KindId, KindName, PathSegment};
use core::time::Duration;

/// Where the program's lifecycle stands (ADR-0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The host has not started it: no stage has been sent.
    Waiting,
    /// It has started, and nothing has asked it to stop.
    Running,
    /// The host or an actor asked it to stop: `Stop` is sent, or will be once a cycle under way has finished.
    Stopping,
    /// `Stop` has finished: the host drops the engine.
    Stopped,
}

/// What the engine keeps of its lifecycle between steps.
pub(super) struct Staging {
    lifecycle: Lifecycle,
    under_way: UnderWay,
    /// How many stages have been sent: the number of the last one's chain.
    sent: u64,
    /// The stages of the chassis's cycle, in the order they run.
    cycle: Vec<Cycling>,
    /// Whether the host has said a cycle is due since the last one began, and at what time.
    due: Due,
    /// The host's time for the last cycle begun, or the steps' time when `Start` was sent, before the first.
    ticked: Time,
    /// The sum of every `Tick`'s delta, in nanoseconds.
    elapsed_nanos: u64,
    /// The time of the cycle under way, or of the last: counted once a cycle, as it begins, for every stage of it.
    time: Cycled,
    /// The most one `Tick`'s delta may be, in nanoseconds: a longer stall slows the program's time.
    delta_limit_nanos: u64,
    /// The subscribers `Stop` was asked of, each by its own count of the request: none before it is sent. `Stop` has
    /// finished only once none of them owes it.
    draining: Vec<(Seated, u64)>,
}

/// Whether a cycle is due.
#[derive(Clone, Copy)]
enum Due {
    /// None is: the host has said none is due since the last began.
    Not,
    /// One is, and the host's time for it is `at`: the last the host said, when it said one was due more than once
    /// while a cycle ran.
    At(Time),
    /// One is, held: its `Tick`'s delta is this many nanoseconds exactly, measured from no host time (ADR-0045).
    By(u64),
}

/// The stage whose chain the engine waits on, if any.
#[derive(Clone, Copy)]
enum UnderWay {
    /// None: the lifecycle waits on the host, or has stopped.
    Nothing,
    /// A stage, standing `at` in the lifecycle, whose chain begins at `root`.
    Stage { root: MailId, at: At },
}

/// Where a stage under way stands in the lifecycle.
#[derive(Clone, Copy)]
enum At {
    Start,
    /// The cycle's stage of this place in it.
    Cycle(usize),
    Stop,
}

/// What each `Tick`'s delta is limited to unless the host says otherwise: a quarter of a second.
const DELTA_LIMIT: Duration = Duration::from_millis(250);

impl Staging {
    /// A lifecycle the host has not started, with no cycle.
    pub(super) fn new() -> Self {
        Self {
            lifecycle: Lifecycle::Waiting,
            under_way: UnderWay::Nothing,
            sent: 0,
            cycle: Vec::new(),
            due: Due::Not,
            ticked: Time::ZERO,
            elapsed_nanos: 0,
            time: Cycled { delta_nanos: 0, elapsed_nanos: 0 },
            delta_limit_nanos: nanos(DELTA_LIMIT),
            draining: Vec::new(),
        }
    }
}

/// Whether a lifecycle whose chassis's cycle is `cycle` runs the stage of kind `stage`: `Start`, `Stop`, or a stage of
/// the cycle (ADR-0020).
pub(super) fn runs(cycle: &[Cycling], stage: KindId) -> bool {
    let is_start = stage.same(KindId::of::<Start>());
    let is_stop = stage.same(KindId::of::<Stop>());
    let is_cycled = cycle.iter().any(|cycling| cycling.kind.same(stage));

    is_start || is_stop || is_cycled
}

impl Staging {
    /// The stages of the chassis's cycle, in the order they run.
    pub(super) fn cycle(&self) -> &[Cycling] {
        &self.cycle
    }
}

/// `duration` in nanoseconds, or the most a `u64` holds.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The name stages are sent under, which the engine keeps for them: no root is made at it, so no actor has the id
/// their chains begin at.
pub(super) const LIFECYCLE: KindName = KindName::new("aether.lifecycle");

/// The id every stage's chain begins at: the engine's own, which no actor has.
pub(super) fn lifecycle_id() -> ActorId {
    root_id(&PathSegment::new(LIFECYCLE, &()))
}

impl Engine {
    /// The engine, running the cycle `C` between `Start` and `Stop`: its stages in order, each a barrier (ADR-0020).
    /// Set as the engine is made, before the host starts it. With none set, the lifecycle runs `Start` and `Stop`
    /// alone. A cycle out of its order does not build ([`Cycle`]).
    pub(super) fn with_cycle<C: Cycle>(mut self) -> Self {
        self.staging.cycle = C::stages();

        self
    }

    /// The engine, limiting each `Tick`'s delta to `limit`, a quarter of a second unless set: a longer stall slows the
    /// program's time, and skips none of it (ADR-0020).
    pub(super) fn with_delta_limit(mut self, limit: Duration) -> Self {
        self.staging.delta_limit_nanos = nanos(limit);

        self
    }

    /// Starts the program's lifecycle, for the host, once it has made its roots: sends `Start` to every actor
    /// subscribed to it by then (ADR-0020). It does nothing once the lifecycle has started.
    pub fn start(&mut self) {
        let is_waiting = self.staging.lifecycle == Lifecycle::Waiting;
        if !is_waiting {
            return;
        }
        self.staging.lifecycle = Lifecycle::Running;
        self.staging.ticked = self.now;
        self.send_stage(Cycling::of::<Start>(), At::Start);
    }

    /// Says a cycle is due, for the host, at `at`, the host's time for it: a browser's animation frame's timestamp, a
    /// display's refresh, or a timer's. The first stage of the cycle is sent once nothing is under way; a cycle said to
    /// be due while one runs is not queued, and the next `Tick` covers the time from the last cycle's to the latest
    /// said (ADR-0020). A `Tick`'s delta is the host's time between cycles, not the steps', so it does not change with
    /// how long each cycle's work took: what moves by it moves evenly on a display that shows each cycle evenly.
    pub fn cycle(&mut self, at: Time) {
        self.staging.due = Due::At(at);
        self.advance();
    }

    /// Says a held cycle is due, for the host: one whose `Tick` carries `delta` exactly, neither measured from the
    /// host's time nor limited, as an advance of a held clock chose it (ADR-0045). It is not queued, as no cycle is:
    /// the host says the next is due once [`Self::has_cycled`] says this one has finished.
    pub fn cycle_by(&mut self, delta: Duration) {
        self.staging.due = Due::By(nanos(delta));
        self.advance();
    }

    /// Says the clock runs again from `at`, for the host: the next cycle it says is due counts its delta from `at`, so
    /// the time the clock was held adds nothing to the program's time (ADR-0045).
    pub const fn resume(&mut self, at: Time) {
        self.staging.ticked = at;
    }

    /// Whether the last cycle said to be due has finished, every stage of it, or never will be run, as no cycle is
    /// when the chassis's holds no stage: what a host waits on before it says the next held cycle is due (ADR-0045).
    #[must_use]
    pub const fn has_cycled(&self) -> bool {
        let is_due = !matches!(self.staging.due, Due::Not);
        let is_cycling = matches!(self.staging.under_way, UnderWay::Stage { at: At::Cycle(_), .. });

        self.staging.cycle.is_empty() || !(is_due || is_cycling)
    }

    /// The program's time: the sum of every `Tick`'s delta so far, held cycles' among them (ADR-0020, ADR-0045).
    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.staging.elapsed_nanos)
    }

    /// The most one `Tick`'s delta measured from the host's time may be: a held cycle's past it is refused before it
    /// is asked for, as no actor is written for it (ADR-0045).
    #[must_use]
    pub const fn delta_limit(&self) -> Duration {
        Duration::from_nanos(self.staging.delta_limit_nanos)
    }

    /// Asks the program to stop, for the host: when its window closed, the process was told to end, or the page was
    /// hidden. The engine lets a cycle under way finish, sends `Stop`, and once that has finished its lifecycle is
    /// [`Lifecycle::Stopped`] (ADR-0020). Asking again changes nothing.
    pub fn quit(&mut self) {
        self.stopping();
        self.advance();
    }

    /// Where the program's lifecycle stands.
    #[must_use]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.staging.lifecycle
    }

    /// Whether the chassis's cycle holds any stage: a host whose engine's does not has no cycle to wake for, and sleeps
    /// until something comes in (ADR-0020).
    #[must_use]
    pub fn has_stages(&self) -> bool {
        !self.staging.cycle.is_empty()
    }

    /// Refuses a root of the type `declared` when it subscribes to a stage the lifecycle does not run, before any of
    /// its code runs (ADR-0020).
    pub(super) fn staged(&self, declared: &Declared) -> Result<(), Refused> {
        let unstaged = declared.unstaged(|stage| runs(&self.staging.cycle, stage));

        unstaged.map_or(Ok(()), |stage| Err(Refused::Unstaged(stage)))
    }

    /// Moves the lifecycle on between steps: past a stage whose chain has finished, to the next stage of a cycle under
    /// way, to `Stop` once it is asked for and no cycle is under way, and to a new cycle once one is due. Run after
    /// each step's fold.
    ///
    /// A stage sent that has finished already, as one nothing subscribes to has, is moved past at once: it costs no
    /// step (ADR-0020).
    pub(super) fn advance(&mut self) {
        while self.move_on() {}
    }

    /// Moves the lifecycle on one stage, as [`Self::advance`] says, and answers whether it sent one.
    fn move_on(&mut self) -> bool {
        // A part's request left untaken here is taken at the next look, and asking twice changes nothing.
        let asked_to_stop = self.stages.shards_mut().any(stages::Shard::take_quit);
        if asked_to_stop {
            self.stopping();
        }
        if let UnderWay::Stage { root, at } = self.staging.under_way {
            let is_stop = matches!(at, At::Stop);
            let is_draining = is_stop && self.is_draining();
            if is_draining || !self.has_finished(root) {
                return false;
            }
            self.staging.under_way = UnderWay::Nothing;
            match at {
                At::Stop => {
                    self.staging.lifecycle = Lifecycle::Stopped;
                    return false;
                }
                At::Cycle(place) => {
                    let next = place + 1;
                    if let Some(stage) = self.staging.cycle.get(next).copied() {
                        self.send_stage(stage, At::Cycle(next));
                        return true;
                    }
                }
                At::Start => {}
            }
        }
        match self.staging.lifecycle {
            Lifecycle::Stopping => {
                self.send_stage(Cycling::of::<Stop>(), At::Stop);
                true
            }
            Lifecycle::Running => self.begin_cycle(),
            Lifecycle::Waiting | Lifecycle::Stopped => false,
        }
    }

    /// Sends the cycle's first stage, when one is due and the cycle holds any, and answers whether it did.
    fn begin_cycle(&mut self) -> bool {
        let first = self.staging.cycle.first().copied();
        let Some(first) = first else {
            return false;
        };
        self.staging.time = match self.staging.due {
            Due::Not => return false,
            Due::At(at) => self.cycled(at),
            Due::By(delta_nanos) => self.held(delta_nanos),
        };
        self.staging.due = Due::Not;
        self.send_stage(first, At::Cycle(0));

        true
    }

    /// Whether a subscriber `Stop` was asked of still holds it: it has not answered, nor let it go, nor ended.
    fn is_draining(&self) -> bool {
        self.staging.draining.iter().any(|&(subscriber, request)| {
            let held = self.actors[self.part(subscriber.seat)].get(subscriber);

            held.is_some_and(|held| held.debts.owes(subscriber, request))
        })
    }

    /// Marks the lifecycle as asked to stop, unless it has stopped already.
    fn stopping(&mut self) {
        let is_live = matches!(self.staging.lifecycle, Lifecycle::Waiting | Lifecycle::Running);
        if is_live {
            self.staging.lifecycle = Lifecycle::Stopping;
        }
    }

    /// The time a cycle the host said was due `at` carries: since the last began, limited, and the sum with it.
    fn cycled(&mut self, at: Time) -> Cycled {
        let since = at.nanos().saturating_sub(self.staging.ticked.nanos());
        let delta_nanos = since.min(self.staging.delta_limit_nanos);
        self.staging.ticked = at;
        self.staging.elapsed_nanos = self.staging.elapsed_nanos.saturating_add(delta_nanos);

        Cycled { delta_nanos, elapsed_nanos: self.staging.elapsed_nanos }
    }

    /// The time a held cycle carries: `delta_nanos` exactly, and the sum with it. The host's time is not read, and the
    /// last cycle's is the step's, so a host that says a cycle is due without resuming counts from there.
    fn held(&mut self, delta_nanos: u64) -> Cycled {
        self.staging.ticked = self.now;
        self.staging.elapsed_nanos = self.staging.elapsed_nanos.saturating_add(delta_nanos);

        Cycled { delta_nanos, elapsed_nanos: self.staging.elapsed_nanos }
    }

    /// Sends `stage`, standing `at`, to every actor subscribed to it, as mail from each to itself, in the order of
    /// their ids, all of one chain, and waits on that chain. `Stop` is sent as a request each subscriber asks of
    /// itself, under its own count, so its answer comes back to it as an answer to nothing it waits on, and is
    /// dropped: the engine reads only whether the request is still held. An actor that has gone is passed over and taken out of
    /// the subscribers; one that has ended and is still to run its closing turn is sent the stage, which is dropped
    /// where it would be handed to it, as all mail to an actor that has ended is.
    fn send_stage(&mut self, stage: Cycling, at: At) {
        self.staging.sent += 1;
        let root = MailId { sender: lifecycle_id(), count: self.staging.sent };
        self.staging.under_way = UnderWay::Stage { root, at };
        let time = self.staging.time;

        let kind = stage.kind;
        let subscribers: Vec<_> = self.stages.shard(key(kind)).subscribers(kind).collect();
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        for subscriber in subscribers {
            let sent = self.lives.shard(subscriber.seat.number()).life(subscriber).map(lives::Life::sent);
            let Some(sent) = sent else {
                self.stages.shard_mut(key(kind)).leave(kind, subscriber);
                continue;
            };
            let Ok(bytes) = (stage.write)(&mut arena, time) else {
                continue;
            };
            let count = sent + 1;
            self.lives.fold_records([lives::sent(subscriber, count)], &[]);
            let tied = match at {
                At::Stop => {
                    self.staging.draining.push((subscriber, count));
                    Tie::asks(count)
                }
                At::Start | At::Cycle(_) => Tie::TELL,
            };
            mailed.push(Mailed {
                by: subscriber,
                to: subscriber,
                count,
                kind,
                bytes,
                tie: tied,
                due: RequestDue::Never,
            });
        }
        self.arenas[parity][ROOTS] = arena;
        self.fold_brought(mailed, Vec::new(), parity, |_| root);
    }
}
