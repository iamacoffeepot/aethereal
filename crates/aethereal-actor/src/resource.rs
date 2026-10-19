//! An actor's resources: values that are not data, each with exactly one owner (ADR-0010).

use crate::{Actors, Fault, MailId, Seated, Within};
use aethereal_data::{
    Decode, DecodeError, Encode, EncodeError, Field, Kind, KindName, Plain, Reader, Schema, Shape, Writer,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::{self, Any};
use core::fmt;
use core::marker::PhantomData;

/// A value that is not data, which one actor owns (ADR-0010).
///
/// A type is a resource because its author says so: nothing a kind's fields
/// can spell holds what the value is. It has no methods. It is `Send`, since a
/// native host runs its owner on any of its workers, with what its owner
/// keeps (ADR-0011). Its type is told apart by Rust's own type id, where it is
/// opened, given, or taken.
pub trait Resource: Any + Send {}

/// An actor's own resource of type `R`: its ticket, which crosses [`Within`] and no other boundary (ADR-0010).
///
/// It is neither `Clone` nor `Copy`: there is one handle to a resource. The
/// owner opens it in its own turn.
pub struct Handle<R: Resource> {
    ticket: u64,
    of: PhantomData<fn() -> R>,
}

/// A resource of type `R` on its way to a new owner: its ticket, which crosses [`Actors`] and no other boundary
/// (ADR-0010).
///
/// It may be in mail or a config, and never in an actor's state. The
/// receiver takes it in the turn it arrives in, or it is dropped.
pub struct Given<R: Resource> {
    ticket: u64,
    of: PhantomData<fn() -> R>,
}

impl<R: Resource> Given<R> {
    /// The resource the ticket `ticket` names, on its way.
    const fn new(ticket: u64) -> Self {
        Self { ticket, of: PhantomData }
    }

    /// Its ticket, used up: a given resource is taken once.
    const fn into_ticket(self) -> u64 {
        self.ticket
    }
}

impl<R: Resource> Handle<R> {
    /// The resource the ticket `ticket` names.
    const fn new(ticket: u64) -> Self {
        Self { ticket, of: PhantomData }
    }

    /// Its ticket, used up: a handle given away is no longer held.
    const fn into_ticket(self) -> u64 {
        self.ticket
    }
}

impl<R: Resource> fmt::Debug for Handle<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle<{}>({})", any::type_name::<R>(), self.ticket)
    }
}

impl<R: Resource> fmt::Debug for Given<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Given<{}>({})", any::type_name::<R>(), self.ticket)
    }
}

/// A handle is its ticket in bytes.
impl<R: Resource> Schema for Handle<R> {
    const SHAPE: Shape = Shape::Struct { name: "Handle", fields: &[Field { name: "ticket", shape: u64::SHAPE }] };
}

/// A given resource is its ticket in bytes.
impl<R: Resource> Schema for Given<R> {
    const SHAPE: Shape = Shape::Struct { name: "Given", fields: &[Field { name: "ticket", shape: u64::SHAPE }] };
}

/// An actor's state holds its handles as their tickets. What a ticket read back names is checked where it is used.
impl<R: Resource> Encode<Within> for Handle<R> {
    fn encode(&self, out: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
        self.ticket.encode(out)
    }
}

impl<R: Resource> Decode<Within> for Handle<R> {
    fn decode(input: &mut Reader<'_, Within>) -> Result<Self, DecodeError> {
        Ok(Self::new(u64::decode(input)?))
    }
}

/// A given resource in mail is its ticket in bytes, and the writer notes it, so the resource goes with the mail it is
/// written into: refused by a writer that carries nothing.
impl<R: Resource> Encode<Actors> for Given<R> {
    fn encode(&self, out: &mut Writer<'_, Actors>) -> Result<(), EncodeError> {
        out.offers().carry(self.ticket)?;
        self.ticket.encode(out)
    }
}

impl<R: Resource> Decode<Actors> for Given<R> {
    fn decode(input: &mut Reader<'_, Actors>) -> Result<Self, DecodeError> {
        Ok(Self::new(u64::decode(input)?))
    }
}

/// The reason of a turn that used a handle or a given resource that names nothing of that type it holds, or that has
/// arrived for it with the mail it handles (ADR-0010).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct NotHeld;

impl Kind for NotHeld {
    const NAME: KindName = KindName::new("aether.actor.not_held");
}

/// The reason of a turn that kept, opened, gave, or took a resource where nothing keeps one: in a guest's code, which
/// holds handles and opens none (ADR-0010, ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct KeepsNothing;

impl Kind for KeepsNothing {
    const NAME: KindName = KindName::new("aether.actor.keeps_nothing");
}

/// One resource, held by the engine for an actor or on its way: its ticket, its name, and the value.
pub struct Kept {
    /// The ticket its owner's handle holds: the owner's own count, so two owners' tickets may be the same number.
    ticket: u64,
    name: &'static str,
    value: Box<dyn Any + Send>,
}

impl Kept {
    /// `value`, under the ticket `ticket`.
    fn new<R: Resource>(ticket: u64, value: R) -> Self {
        Self { ticket, name: any::type_name::<R>(), value: Box::new(value) }
    }

    /// Whether it is the resource of type `R` under the ticket `ticket`.
    fn is<R: Resource>(&self, ticket: u64) -> bool {
        let is_ticket = self.ticket == ticket;

        is_ticket && self.value.is::<R>()
    }

    /// What the resource is called.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

/// What an actor keeps, beside it in its seat: what it owns, and what has arrived for it with mail it has not handled
/// yet (ADR-0010). Dropped with the actor, after its closing turn.
///
/// Most actors keep nothing ever, and every actor's seat holds one, so one that has kept nothing holds no more than
/// a pointer's width.
#[derive(Default)]
pub struct Keeping {
    holding: Holding,
}

/// Whether an actor has kept anything yet.
#[derive(Default)]
enum Holding {
    /// It has never owned a resource, nor been given one.
    #[default]
    Never,
    /// It has: what it holds now, and how many it has come to own.
    Holds(Box<Holdings>),
}

/// What an actor that has kept something holds.
#[derive(Default)]
struct Holdings {
    /// How many resources the actor has come to own: the count each new one's ticket is.
    owned: u64,
    held: Vec<Keep>,
}

/// One resource an actor keeps.
enum Keep {
    /// Its own, under its own ticket.
    Mine(Kept),
    /// Given to it by the actor `by`, under that giver's ticket, with the mail `mail`, which it has not handled yet.
    Arriving { by: Seated, mail: MailId, kept: Kept },
}

impl Keeping {
    /// What it holds, holding nothing yet when it never has.
    fn holdings(&mut self) -> &mut Holdings {
        let has_never_held = matches!(self.holding, Holding::Never);
        if has_never_held {
            self.holding = Holding::Holds(Box::default());
        }
        let Holding::Holds(holdings) = &mut self.holding else {
            unreachable!("it holds once it has been given its holdings");
        };

        holdings
    }

    /// What it holds, when it has ever held anything.
    fn held(&mut self) -> Result<&mut Holdings, Fault> {
        match &mut self.holding {
            Holding::Holds(holdings) => Ok(holdings),
            Holding::Never => Err(Fault::bare::<NotHeld>()),
        }
    }

    /// Keeps `value`, under the next ticket, and answers with the handle to it.
    fn keep<R: Resource>(&mut self, value: R) -> Handle<R> {
        self.holdings().keep(value)
    }

    /// Keeps `kept`, a resource given to the actor, under the next ticket, and answers with the handle to it.
    fn adopt<R: Resource>(&mut self, kept: Kept) -> Handle<R> {
        self.holdings().adopt(kept)
    }

    /// The resource `handle` names.
    fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        self.held()?.open(handle)
    }

    /// Lets go of the resource of type `R` its handle's `ticket` names, and answers with it.
    fn give_up<R: Resource>(&mut self, ticket: u64) -> Result<Kept, Fault> {
        self.held()?.give_up::<R>(ticket)
    }

    /// Takes the resource of type `R` the ticket `ticket` names, which arrived from the actor `by` with the mail being
    /// handled, as the actor's own.
    fn take_arrived<R: Resource>(&mut self, ticket: u64, by: Seated) -> Result<Handle<R>, Fault> {
        self.held()?.take_arrived::<R>(ticket, by)
    }

    /// Keeps `kept`, given by the actor `by` with the mail `mail`, until the actor handles that mail: for the engine,
    /// as the mail's step folds.
    pub fn arrive(&mut self, by: Seated, mail: MailId, kept: Kept) {
        self.holdings().held.push(Keep::Arriving { by, mail, kept });
    }

    /// Whether it keeps nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.holding {
            Holding::Never => true,
            Holding::Holds(holdings) => holdings.held.is_empty(),
        }
    }

    /// Drops what arrived with the mail `mail` that the actor did not take as it handled it, and answers with the name
    /// of each: for the engine, after the turn of that mail.
    pub fn drop_untaken(&mut self, mail: MailId) -> Vec<&'static str> {
        self.held().map(|holdings| holdings.drop_untaken(mail)).unwrap_or_default()
    }

    /// Takes out what arrived for the actor with mail it never handled, each with the actor that gave it: for the
    /// engine, as the actor leaves its seat. What it owns stays, and is dropped with it.
    pub fn take_arriving(&mut self) -> Vec<(Seated, Kept)> {
        self.held().map(Holdings::take_arriving).unwrap_or_default()
    }
}

impl Holdings {
    /// Keeps `value`, under the next ticket, and answers with the handle to it.
    fn keep<R: Resource>(&mut self, value: R) -> Handle<R> {
        self.owned += 1;
        self.held.push(Keep::Mine(Kept::new(self.owned, value)));

        Handle::new(self.owned)
    }

    /// Keeps `kept`, a resource given to the actor, under the next ticket, and answers with the handle to it.
    fn adopt<R: Resource>(&mut self, mut kept: Kept) -> Handle<R> {
        self.owned += 1;
        kept.ticket = self.owned;
        self.held.push(Keep::Mine(kept));

        Handle::new(self.owned)
    }

    /// The resource `handle` names.
    fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        self.held
            .iter_mut()
            .find_map(|keep| match keep {
                Keep::Mine(kept) if kept.ticket == handle.ticket => kept.value.downcast_mut::<R>(),
                _ => None,
            })
            .ok_or_else(Fault::bare::<NotHeld>)
    }

    /// Lets go of the resource of type `R` its handle's `ticket` names, and answers with it.
    fn give_up<R: Resource>(&mut self, ticket: u64) -> Result<Kept, Fault> {
        let index = self
            .held
            .iter()
            .position(|keep| matches!(keep, Keep::Mine(kept) if kept.is::<R>(ticket)))
            .ok_or_else(Fault::bare::<NotHeld>)?;
        let Keep::Mine(kept) = self.held.remove(index) else {
            unreachable!("the position found is of a resource the actor owns");
        };

        Ok(kept)
    }

    /// Takes the resource of type `R` the ticket `ticket` names, which arrived from the actor `by` with the mail being
    /// handled, as the actor's own.
    fn take_arrived<R: Resource>(&mut self, ticket: u64, by: Seated) -> Result<Handle<R>, Fault> {
        let index = self
            .held
            .iter()
            .position(
                |keep| matches!(keep, Keep::Arriving { by: from, kept, .. } if *from == by && kept.is::<R>(ticket)),
            )
            .ok_or_else(Fault::bare::<NotHeld>)?;
        let Keep::Arriving { kept, .. } = self.held.remove(index) else {
            unreachable!("the position found is of a resource that arrived");
        };

        Ok(self.adopt(kept))
    }

    /// Drops what arrived with the mail `mail` that the actor did not take as it handled it, and answers with the name
    /// of each.
    fn drop_untaken(&mut self, mail: MailId) -> Vec<&'static str> {
        let mut untaken = Vec::new();
        self.held.retain(|keep| match keep {
            Keep::Arriving { mail: with, kept, .. } if *with == mail => {
                untaken.push(kept.name);
                false
            }
            _ => true,
        });

        untaken
    }

    /// Takes out what arrived for the actor with mail it never handled, each with the actor that gave it.
    fn take_arriving(&mut self) -> Vec<(Seated, Kept)> {
        let mut arriving = Vec::new();
        let mut index = 0;
        while index < self.held.len() {
            let is_arriving = matches!(self.held[index], Keep::Arriving { .. });
            if is_arriving {
                if let Keep::Arriving { by, kept, .. } = self.held.remove(index) {
                    arriving.push((by, kept));
                }
            } else {
                index += 1;
            }
        }

        arriving
    }
}

/// Who gave a resource that is on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Giver {
    /// The chassis, to a root in its config (ADR-0010).
    Chassis,
    /// The actor this names, in one of its turns.
    Actor(Seated),
}

/// Where a resource given in a turn is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// Nowhere yet: no mail carries it. It is dropped at the end of its turn unless a child's `begin` takes it.
    Nowhere,
    /// To the actor `to`, with the mail `mail`.
    To { to: Seated, mail: MailId },
}

/// A resource given in a turn, held by the engine on its way to a new owner (ADR-0010).
pub struct Gift {
    pub by: Giver,
    pub on: Carried,
    pub kept: Kept,
}

impl Gift {
    /// `value`, given by the chassis under the ticket `ticket`: what the engine holds for a root's config.
    #[must_use]
    pub fn handed<R: Resource>(ticket: u64, value: R) -> (Self, Given<R>) {
        (Self { by: Giver::Chassis, on: Carried::Nowhere, kept: Kept::new(ticket, value) }, Given::new(ticket))
    }

    /// Whether it is the resource `ticket` names, given by `by` and carried nowhere yet.
    fn is_loose(&self, by: Giver, ticket: u64) -> bool {
        let is_giver = self.by == by;
        let is_loose = self.on == Carried::Nowhere;

        is_giver && is_loose && self.kept.ticket == ticket
    }

    /// Its ticket, under its giver.
    #[must_use]
    pub const fn ticket(&self) -> u64 {
        self.kept.ticket
    }
}

/// What a turn's calls on resources reach: the actor's own keeping, or nothing, for code that keeps nothing.
pub enum Keeps<'a> {
    /// What the engine keeps for the actor.
    Here(&'a mut Keeping),
    /// Nothing: a guest's code keeps and opens nothing (ADR-0010, ADR-0015).
    Nothing,
}

impl Keeps<'_> {
    /// The actor's keeping.
    fn keeping(&mut self) -> Result<&mut Keeping, Fault> {
        match self {
            Self::Here(keeping) => Ok(keeping),
            Self::Nothing => Err(Fault::bare::<KeepsNothing>()),
        }
    }

    /// Keeps `value` as the actor's own.
    pub(crate) fn keep<R: Resource>(&mut self, value: R) -> Result<Handle<R>, Fault> {
        Ok(self.keeping()?.keep(value))
    }

    /// The resource `handle` names.
    pub(crate) fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        self.keeping()?.open(handle)
    }

    /// Lets go of the resource `handle` names, for the actor `me` to give it: answers with the gift, and the given
    /// resource that names it.
    pub(crate) fn give<R: Resource>(&mut self, me: Seated, handle: Handle<R>) -> Result<(Gift, Given<R>), Fault> {
        let kept = self.keeping()?.give_up::<R>(handle.into_ticket())?;
        let given = Given::new(kept.ticket);

        Ok((Gift { by: Giver::Actor(me), on: Carried::Nowhere, kept }, given))
    }

    /// Takes the resource `given` names, which came from the actor `by` with the mail being handled.
    pub(crate) fn take_arrived<R: Resource>(&mut self, given: Given<R>, by: Seated) -> Result<Handle<R>, Fault> {
        self.keeping()?.take_arrived::<R>(given.into_ticket(), by)
    }

    /// Takes the resource `given` names out of `gifts`, given by `by` and carried nowhere, as the actor's own: what a
    /// `begin` takes from its config.
    pub(crate) fn take_given<R: Resource>(
        &mut self,
        given: Given<R>,
        by: Giver,
        gifts: &mut Vec<Gift>,
    ) -> Result<Handle<R>, Fault> {
        let keeping = self.keeping()?;
        let ticket = given.into_ticket();
        let found = gifts.iter().position(|gift| gift.is_loose(by, ticket) && gift.kept.value.is::<R>());
        let index = found.ok_or_else(Fault::bare::<NotHeld>)?;

        Ok(keeping.adopt(gifts.remove(index).kept))
    }
}

/// Marks each gift `tickets` name, given by `by` and carried nowhere, as carried to `to` with the mail `mail`: refused,
/// marking none, when one names no such gift.
pub fn carry(gifts: &mut [Gift], tickets: &[u64], by: Seated, to: Seated, mail: MailId) -> Result<(), EncodeError> {
    let by = Giver::Actor(by);
    let every = tickets.iter().all(|ticket| gifts.iter().any(|gift| gift.is_loose(by, *ticket)));
    if !every {
        return Err(EncodeError::NotCarried { name: "Given" });
    }
    for ticket in tickets {
        if let Some(gift) = gifts.iter_mut().find(|gift| gift.is_loose(by, *ticket)) {
            gift.on = Carried::To { to, mail };
        }
    }

    Ok(())
}
