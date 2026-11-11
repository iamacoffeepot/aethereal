//! Moving guests onto their kinds' new code between steps: each actor whose batch stopped as it answered that it moves
//! now, and each actor with no mail of a kind whose head moved, asked once (ADR-0054).

use super::{Engine, Held, Holding, parity};
use crate::heads::HEADS;
use crate::{Context, lives, log};
use aethereal_actor::{Behind, Effect, Fault, Level, MailId, Moving, Seated, Upgraded, What};
use aethereal_data::KindName;
use std::mem;

impl Engine {
    /// Moves each actor whose batch stopped in the last step as it answered that it moves onto its kind's head now,
    /// then asks each live actor with no mail of a kind whose head moved since the engine last asked, once, in a call
    /// with no turn: between steps, where the engine holds every part (ADR-0054). An idle actor would otherwise never be
    /// asked, and a publish would never settle.
    pub(super) fn upgrade_behind(&mut self) {
        for me in mem::take(&mut self.moving) {
            self.upgrade(me, Behind::Answered);
        }

        let published = self.heads.shard(HEADS).published();
        let has_moved = published > self.asked_through;
        if !has_moved {
            return;
        }
        let kinds: Vec<KindName> = self.heads.shard(HEADS).since(self.asked_through).cloned().collect();
        self.asked_through = published;
        let mut idle = Vec::new();
        for part in 0..self.seating.len() {
            let (streams, living) = (self.streams.at(part), self.lives.at(part));
            for (me, held) in self.actors[part].seated(part) {
                let Holding::Here(actor) = &held.actor else {
                    continue;
                };
                let is_of_kind = kinds.contains(actor.declared().name());
                let is_live = living.life(me).is_some_and(lives::Life::is_live);
                let waits = streams.holds_for(me);
                let is_idle = is_of_kind && is_live && !waits;
                if is_idle {
                    idle.push(me);
                }
            }
        }
        for me in idle {
            self.upgrade(me, Behind::Idle);
        }
    }

    /// Reaches the actor `me` names, in its part, to move onto its kind's head as it stands, for why `behind` says, and
    /// takes in what became of it: its new type, the lines its move logs as its own, each tagged with the publish
    /// that made the head, and the actors that ended as a call broke their instance. Nothing when it is not live, or
    /// its kind has no head.
    fn upgrade(&mut self, me: Seated, behind: Behind) {
        let part = self.part(me.seat);
        let is_live = self.lives.at(part).life(me).is_some_and(lives::Life::is_live);
        if !is_live {
            return;
        }
        let (number, upgraded) = {
            let Self { actors, instances, heads, .. } = self;
            let Some(Held { actor: Holding::Here(actor), .. }) = actors[part].get_mut(me) else {
                return;
            };
            let Some(head) = heads.shard(HEADS).head(actor.declared().name()) else {
                return;
            };
            let moving = Moving { behind, head: head.blob(), instances: &mut instances[part] };

            (head.number(), actor.upgrade(moving))
        };
        self.stats.measure(part, &self.instances[part]);

        match upgraded {
            Upgraded::Stays => {}
            Upgraded::Moved { declared, absent } => {
                if let Some(sitting) = self.seating[part].seated.get_mut(me) {
                    sitting.kind = declared.id();
                }
                self.table.moved(me.id, declared);
                let absent = if absent.is_empty() {
                    String::from("none")
                } else {
                    absent.join(", ")
                };
                self.say(me, Level::Info, format!("moved to publish {number}; absent: {absent}"));
            }
            Upgraded::Parked { reason, ended } => {
                self.say(me, Level::Error, format!("did not move to publish {number}: {reason}; parked"));
                self.end_between(me, ended);
            }
            Upgraded::Broke { ended } => self.end_between(me, ended),
        }
    }

    /// Logs `text` at `level` as the actor `me`'s own line, between steps (ADR-0044, ADR-0054).
    fn say(&mut self, me: Seated, level: Level, text: String) {
        self.log.fold_records([log::said(me, level, text)], &[]);
    }

    /// Ends each of `ended` with its fault, as a failed turn ends its actor, between steps, for the move of the actor
    /// `by`: what a call into an instance broke as it moved (ADR-0015). Their watchers are shown their endings, and
    /// their closing turns are run in the next step.
    fn end_between(&mut self, by: Seated, ended: Vec<(Seated, Fault)>) {
        if ended.is_empty() {
            return;
        }
        let effects: Vec<Effect> =
            ended.into_iter().map(|(actor, fault)| Effect::new(actor, What::Failed(fault))).collect();
        // Their endings are a chain of their own, begun by the move.
        let context = Context { root: MailId { sender: by.id, count: 0 }, depth: 1 };
        self.fold_turn(&effects, Vec::new(), context);
        let written = &self.arenas[parity(self.step)];
        self.watches.fold(&effects, &context, written);

        let mut forward = mem::take(&mut self.spare.forward);
        for shard in self.watches.shards_mut() {
            shard.settle(&mut forward);
        }
        self.hand_on(forward);
        for part in 0..self.seating.len() {
            self.expected[part] = self.expect(part);
        }
        self.waiting.extend(0..self.seating.len());
    }
}
