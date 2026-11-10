//! The root that holds an engine's tools, whether or not it links to a hub (ADR-0053).

mod kinds;
mod settings;
#[cfg(test)]
mod tests;

pub use kinds::{CallTool, ListTools, Sheets, Subscribe};
pub use settings::ToolsSettings;

use crate::{Call, Called, Holder, Offer, Offered, Table, Tooling, Withdraw};
use aethereal_actor::{
    Actor, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held, Level,
    Subscribers, ToAny, ToSender, Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::Bootstrap;
use std::collections::BTreeMap;

/// What the root keeps for a call it asked of a worker: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.forwarding")]
pub struct Forwarding {
    number: u64,
}

/// The engine's tools (ADR-0053): the [`Holder`] every engine lists, which any actor may [`Offer`] tools at its typed
/// path, `path::<Tools>(())`, naming a worker that speaks [`Tooling`]. It checks each offer with the shared [`Table`],
/// answers [`Offered`] in the turn that asks, and watches each worker, as `Mcp` does in a hub, so a worker's end drops
/// its tools.
///
/// Any actor may ask it to [`ListTools`], answered with every sheet as [`Sheets`], or [`Subscribe`], to be sent the
/// `Sheets` as they stand and again after each change. A [`CallTool`] is asked of the worker that offered the tool,
/// due when the call is (ADR-0041), and answered with what the worker answered: a call of a revision it has changed
/// since, or of a tool it does not hold, is refused at once, and a worker that does not answer is refused saying why.
///
/// It needs no hub. Where one is wanted, `Link` subscribes to it, writes each set to the hub, and asks it for each
/// call the hub sends (ADR-0038).
#[derive(Bound, Bootstrap)]
#[kind(name = "aether.tools")]
pub struct Tools {
    settings: ToolsSettings,
    table: Table,
    /// The revision of what it holds: raised with every change, and never reset.
    revision: u64,
    subscribers: Subscribers<(Sheets,)>,
    /// Each call asked of a worker, by the root's own number, held until the worker answers.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

impl ChildOf for Tools {
    type Parent = Top;
}

/// It holds the tools an engine's own program offers.
impl Holder for Tools {}

/// Asked at its typed path, it answers an offer in the same turn.
impl Carries<Offer> for Tools {
    type Answers = (Offered,);
}

/// Asked at its typed path, it answers with every sheet it holds.
impl Carries<ListTools> for Tools {
    type Answers = (Sheets,);
}

/// Asked at its typed path, it answers with what the tool's worker answered.
impl Carries<CallTool> for Tools {
    type Answers = (Called,);
}

impl Actor for Tools {
    type Config = ToolsSettings;
    type Key = ();
    type Handles = (Offer, Withdraw, Ended, ListTools, Subscribe, CallTool, Answer<Called, Forwarding>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(settings: ToolsSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self {
            settings,
            table: Table::new(),
            revision: 0,
            subscribers: Subscribers::new(),
            calls: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Offer> for Tools {
    type Sends = (ToSender<Offered>, ToAny<(Sheets,), Sheets>);

    /// Holds the offer, when its tools fit in the table, and sends the set with them to each subscriber.
    fn handle(&mut self, offer: Offer, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let room = usize::try_from(self.settings.most_tools).unwrap_or(usize::MAX);
        let worker = offer.worker;
        let offered = self.table.take(offer.tools, worker, room);
        if offered == Offered::Yes {
            turn.watch(worker);
            self.changed(turn)?;
        }
        turn.reply(&offered)?;

        Ok(())
    }
}

impl Handles<Withdraw> for Tools {
    type Sends = (ToAny<(Sheets,), Sheets>,);

    /// Stops holding the tool, and sends the set without it to each subscriber.
    fn handle(&mut self, withdraw: Withdraw, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = self.table.len();
        self.table.withdraw(&withdraw.tool, withdraw.worker);
        let is_changed = self.table.len() < held;
        if is_changed {
            self.changed(turn)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Tools {
    type Sends = (ToAny<(Sheets,), Sheets>,);

    /// A subscriber has ended, and is dropped; or a worker has, and its tools are dropped and the set sent.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.subscribers.ended(&ended);
        let held = self.table.len();
        self.table.ended(ended.actor());
        let is_changed = self.table.len() < held;
        if is_changed {
            self.changed(turn)?;
        }

        Ok(())
    }
}

impl Handles<ListTools> for Tools {
    type Sends = (ToSender<Sheets>,);

    fn handle(&mut self, _: ListTools, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&self.sheets())?;

        Ok(())
    }
}

/// Adds the sender, and sends it the set as it stands: nothing, when it is subscribed already.
impl Handles<Subscribe> for Tools {
    type Sends = (ToAny<(Sheets,), Sheets>,);

    fn handle(&mut self, _: Subscribe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(subscriber) = turn.cast::<(Sheets,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle `Sheets` asked to be sent them");
            return Ok(());
        };
        let added = self.subscribers.subscribe(turn, subscriber);
        if !added {
            return Ok(());
        }
        turn.to(subscriber).send(&self.sheets())?;

        Ok(())
    }
}

impl Handles<CallTool> for Tools {
    type Sends = (ToSender<Called>, Asks<Tooling, Call, Forwarding>);

    /// Asks the call of the worker that offered the tool, holding it until the worker answers: the ask is made in this
    /// turn, so it is due when the call is (ADR-0041). A call of a revision it has changed since is refused at once, in
    /// the turn that looks the tool up, so no tool offered since is called with bytes written for another schema; and a
    /// tool it does not hold is refused at once.
    fn handle(&mut self, call: CallTool, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let CallTool { tool, revision, input } = call;
        if revision != self.revision {
            let changed = format!("the engine's tools have changed since revision {revision}: read them again");
            turn.reply(&Called::Refused(changed))?;
            return Ok(());
        }
        let Some(held) = self.table.tool(&tool) else {
            turn.reply(&Called::Refused(format!("the engine holds no tool named {tool}")))?;
            return Ok(());
        };
        let worker = held.worker;
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.to(worker).ask(&Call { tool, input }, &Forwarding { number })?;
        self.calls.insert(number, held);

        Ok(())
    }
}

impl Handles<Answer<Called, Forwarding>> for Tools {
    type Sends = ();

    /// The worker's answer, answered to whoever called: a worker that did not answer is refused, saying why.
    fn handle(
        &mut self,
        (answered, Forwarding { number }): (Answered<Called>, Forwarding),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(called) => called,
            Answered::No(Unanswered::Expired) => Called::Refused(String::from("the tool did not answer in time")),
            Answered::No(why) => Called::Refused(format!("the tool did not answer: {why:?}")),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Tools {
    /// Every tool it holds, as the set of its revision.
    fn sheets(&self) -> Sheets {
        Sheets { revision: self.revision, tools: self.table.sheets() }
    }

    /// What it holds has changed: the revision is raised, and the set sent to each subscriber.
    fn changed<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(Sheets,), Sheets>, I>,
    {
        self.revision += 1;
        turn.to(&self.subscribers).send::<_, I>(&self.sheets())?;

        Ok(())
    }
}
