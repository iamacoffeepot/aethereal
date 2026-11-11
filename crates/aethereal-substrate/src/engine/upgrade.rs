//! Guests moving onto their kinds' new code: an actor behind its kind's head is asked before each batch of its turns,
//! on the worker that runs its part, and moved there when it answers `Now`; an actor with no mail is asked once between
//! steps, after its kind's head moves (ADR-0054).

#[cfg(any(test, feature = "harness"))]
use super::outside;
use super::{Engine, Held, Holding, Lists, Reading};
use crate::heads::{self, Became, HEADS};
use crate::{Context, lives, log};
use aethereal_actor::{Effect, Fault, Instances, Level, MailId, Moving, Runs, Seated, Upgraded, What};
use aethereal_data::{ActorId, KindId, KindName};

/// What became of an actor asked, before a batch of its turns, whether to move onto its kind's head (ADR-0054).
pub(super) enum Upgrading {
    /// It runs the batch on the code it ran: it was not asked, answered `Later`, or did not move.
    Stays,
    /// It moved, and runs the batch on the code whose type has this kind.
    Moved(KindId),
    /// A call into the instance of its code broke, so it ended: the batch's items are not handed to it.
    Ended,
}

/// What the engine keeps of what became of an actor reached to move onto its kind's head: the change to its type, the
/// endings of the actors a call that broke ended, and the line its move logs as its own (ADR-0054).
struct Kept {
    effects: Vec<Effect>,
    said: Vec<(u64, (ActorId, log::Logged))>,
}

impl Kept {
    /// What is kept of `upgraded`, what became of the actor `me` reached to move onto the head the publish numbered
    /// `number` made: each line tagged with that number.
    fn of(me: Seated, upgraded: Upgraded, number: u64) -> Self {
        let failed = |ended: Vec<(Seated, Fault)>| -> Vec<Effect> {
            ended.into_iter().map(|(actor, fault)| Effect::new(actor, What::Failed(fault))).collect()
        };
        let (effects, said) = match upgraded {
            Upgraded::Unasked | Upgraded::Later => (Vec::new(), Vec::new()),
            Upgraded::Moved { declared, absent } => {
                let absent = if absent.is_empty() {
                    String::from("none")
                } else {
                    absent.join(", ")
                };
                let line = log::said(me, Level::Info, format!("moved to publish {number}; absent: {absent}"));

                (vec![Effect::new(me, What::Moved { declared })], vec![line])
            }
            Upgraded::Parked { reason, ended } => {
                let line = log::said(me, Level::Error, format!("did not move to publish {number}: {reason}; parked"));

                (failed(ended), vec![line])
            }
            Upgraded::BrokeAsking { ended } | Upgraded::BrokeMoving { ended } => (failed(ended), Vec::new()),
        };

        Self { effects, said }
    }
}

impl Upgrading {
    /// What becomes of the batch of an actor reached to move onto its kind's head, as `upgraded` says.
    const fn of(upgraded: &Upgraded) -> Self {
        match upgraded {
            Upgraded::Moved { declared, .. } => Self::Moved(declared.id()),
            Upgraded::BrokeAsking { .. } | Upgraded::BrokeMoving { .. } => Self::Ended,
            Upgraded::Unasked | Upgraded::Later | Upgraded::Parked { .. } => Self::Stays,
        }
    }
}

impl Reading<'_> {
    /// Before a batch of the turns of `actor`, which the actor `me` names: asks it whether to move onto its kind's
    /// head, when its code is behind the head as the step began and it is not parked there, and moves it at once when
    /// it answers `Now`, in its part, whose `instances` are lent, so the batch runs on the code it then runs. What the
    /// engine keeps of it is routed into `lists` in `context`, the chain of the batch's first item, and is taken in at
    /// the fold as every change a step makes is: the table learns its new type there (ADR-0011, ADR-0054).
    pub(super) fn upgrade(
        &self,
        me: Seated,
        actor: &mut dyn Runs,
        instances: &mut Instances,
        context: Context,
        lists: &mut Lists,
    ) -> Upgrading {
        let Some(head) = self.heads.shard(HEADS).head(actor.declared().name()) else {
            return Upgrading::Stays;
        };
        let kind = actor.declared().name();
        let upgraded = actor.upgrade(Moving { head: head.blob(), instances });
        // Most batches are of an actor on its kind's head.
        let Some(became) = Became::of(&upgraded) else {
            return Upgrading::Stays;
        };
        #[cfg(any(test, feature = "harness"))]
        self.recording.asked_to_move(me, &upgraded, head.number(), &mut lists.asked_to_move);
        let upgrading = Upgrading::of(&upgraded);
        let (key, record) = heads::asked(me.id, kind.clone(), head.number(), became);
        lists.routes.heads.put(key, record);

        let Kept { effects, said } = Kept::of(me, upgraded, head.number());
        for (key, record) in said {
            lists.routes.log.put(key, record);
        }
        for effect in &effects {
            lists.routes.route(effect, &context);
        }

        upgrading
    }
}

impl Engine {
    /// Asks each live actor with no mail of a kind whose head moved since the engine last asked, once, whether to move
    /// onto it, and moves it when it answers `Now`: between steps, where the engine holds every part. An idle actor
    /// would otherwise never be asked, and never move. Each live actor of such a kind that has mail and is behind the
    /// head is recorded as still to be asked, before its next batch, so who has yet to answer is known (ADR-0054).
    pub(super) fn ask_idle(&mut self) {
        let heads = self.heads.shard(HEADS);
        let published = heads.published();
        let has_moved = published > self.asked_through;
        if !has_moved {
            return;
        }
        let kinds: Vec<KindName> = heads.since(self.asked_through).cloned().collect();
        self.asked_through = published;
        let mut idle = Vec::new();
        let mut unasked = Vec::new();
        for part in 0..self.seating.len() {
            let (streams, living) = (self.streams.at(part), self.lives.at(part));
            for (me, held) in self.actors[part].seated(part) {
                let Holding::Here(actor) = &held.actor else {
                    continue;
                };
                let kind = actor.declared().name();
                let is_of_kind = kinds.contains(kind);
                let is_live = living.life(me).is_some_and(lives::Life::is_live);
                let waits = streams.holds_for(me);
                let is_reached = is_of_kind && is_live;
                if !is_reached {
                    continue;
                }
                if !waits {
                    idle.push(me);
                    continue;
                }
                let Some(head) = heads.head(kind) else {
                    continue;
                };
                if actor.is_behind(head.blob().id()) {
                    unasked.push(heads::asked(me.id, kind.clone(), head.number(), Became::Unasked));
                }
            }
        }
        self.heads.fold_records(unasked, &[]);
        for me in idle {
            self.ask_once(me);
        }
    }

    /// Asks the actor `me` names, in its part, whether to move onto its kind's head, and moves it when it answers
    /// `Now`, taking in at once what the engine keeps of it: its new type, its line, and the endings of actors a call
    /// that broke ended. Nothing when it is not live, as an earlier ask ended it, or its kind has no head.
    fn ask_once(&mut self, me: Seated) {
        let part = self.part(me.seat);
        let is_live = self.lives.at(part).life(me).is_some_and(lives::Life::is_live);
        if !is_live {
            return;
        }
        let (kind, number, upgraded) = {
            let Self { actors, instances, heads, .. } = self;
            let Some(Held { actor: Holding::Here(actor), .. }) = actors[part].get_mut(me) else {
                return;
            };
            let kind = actor.declared().name();
            let Some(head) = heads.shard(HEADS).head(kind) else {
                return;
            };
            let moving = Moving { head: head.blob(), instances: &mut instances[part] };

            (kind, head.number(), actor.upgrade(moving))
        };
        self.stats.measure(part, &self.instances[part]);
        #[cfg(any(test, feature = "harness"))]
        outside::Recording::of(self.recording, self.step).asked_to_move(me, &upgraded, number, &mut self.asked_to_move);
        if let Some(became) = Became::of(&upgraded) {
            self.heads.fold_records([heads::asked(me.id, kind.clone(), number, became)], &[]);
        }
        if let Upgrading::Moved(kind) = Upgrading::of(&upgraded)
            && let Some(sitting) = self.seating[part].seated.get_mut(me)
        {
            sitting.kind = kind;
        }

        let Kept { effects, said } = Kept::of(me, upgraded, number);
        self.log.fold_records(said, &[]);
        self.fold_between(me, &effects);
    }

    /// Folds `effects`, what the engine keeps of the ask it made of the actor `by` between steps, as a turn's are: its
    /// new type into the table, and each ending, whose closing turn runs in the next step (ADR-0015, ADR-0054).
    fn fold_between(&mut self, by: Seated, effects: &[Effect]) {
        if effects.is_empty() {
            return;
        }
        // They are a chain of their own, begun by the ask.
        let context = Context { root: MailId { sender: by.id, count: 0 }, depth: 1 };
        self.fold_turn(effects, Vec::new(), context);
        // An ending's watchers are shown it once its closing turn has run, in the next step.
        for part in 0..self.seating.len() {
            self.expected[part] = self.expect(part);
        }
        self.waiting.extend(0..self.seating.len());
    }
}
