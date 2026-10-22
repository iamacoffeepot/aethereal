//! An actor's resources: values that are not data, each with exactly one owner (ADR-0010).

use crate::{Above, Actors, Fault, MailId, Seated, Within};
use aethereal_data::{Decode, DecodeError, Encode, EncodeError, Field, Plain, Reader, Schema, Shape, Writer};
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
    pub(crate) const fn new(ticket: u64) -> Self {
        Self { ticket, of: PhantomData }
    }

    /// Its ticket, used up: a given resource is taken once.
    pub(crate) const fn into_ticket(self) -> u64 {
        self.ticket
    }
}

impl<R: Resource> Handle<R> {
    /// The resource the ticket `ticket` names.
    pub(crate) const fn new(ticket: u64) -> Self {
        Self { ticket, of: PhantomData }
    }

    /// Its ticket, used up: a handle given away is no longer held.
    pub(crate) const fn into_ticket(self) -> u64 {
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
#[kind(name = "aether.actor.not_held")]
pub struct NotHeld;

/// The reason of a turn that kept or opened a resource where nothing is kept: in a guest's code, which holds handles
/// and opens none, and makes no resource to keep (ADR-0010, ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.actor.keeps_nothing")]
pub struct KeepsNothing;

/// One resource, held by the engine for an actor or on its way: its ticket, its name, and the value.
pub struct Kept {
    /// The ticket every handle to it and every given resource that names it holds, which no other resource of the
    /// engine has had: it is the same from when it is kept until it is dropped, whoever owns it.
    ticket: u64,
    name: &'static str,
    value: Box<dyn Any + Send>,
}

impl Kept {
    /// `value`, under the ticket `ticket`.
    pub(crate) fn new<R: Resource>(ticket: u64, value: R) -> Self {
        Self { ticket, name: any::type_name::<R>(), value: Box::new(value) }
    }

    /// Whether it is a resource of type `R`.
    fn is<R: Resource>(&self) -> bool {
        self.value.is::<R>()
    }

    /// What the resource is called.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Its ticket.
    #[must_use]
    pub const fn ticket(&self) -> u64 {
        self.ticket
    }
}

/// Whatever type a resource is: for a guest's host, which holds a guest to the tickets it claims and cannot know the
/// types the guest names them as. The type is checked where native code opens or takes it.
const fn any_type(_: &Kept) -> bool {
    true
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
    /// It has: what it holds now, behind a pointer, so an actor that never held anything holds no more than one.
    Holds(Box<Holdings>),
}

/// What an actor that has held a resource holds now.
#[derive(Default)]
struct Holdings {
    held: Vec<Keep>,
}

/// One resource an actor keeps.
enum Keep {
    /// Its own.
    Mine(Kept),
    /// Given to it by the actor `by` with the mail `mail`, which it has not handled yet.
    Arriving { by: Seated, mail: MailId, kept: Kept },
}

impl Keep {
    /// The ticket of the resource kept.
    const fn ticket(&self) -> u64 {
        match self {
            Self::Mine(kept) | Self::Arriving { kept, .. } => kept.ticket,
        }
    }
}

impl Keeping {
    /// What it holds, holding nothing yet when it never has.
    fn holdings(&mut self) -> &mut Vec<Keep> {
        let has_never_held = matches!(self.holding, Holding::Never);
        if has_never_held {
            self.holding = Holding::Holds(Box::default());
        }
        let Holding::Holds(holdings) = &mut self.holding else {
            unreachable!("it holds once it has been given its holdings");
        };

        &mut holdings.held
    }

    /// What it holds, when it has ever held anything.
    fn held(&mut self) -> Option<&mut Vec<Keep>> {
        match &mut self.holding {
            Holding::Holds(holdings) => Some(&mut holdings.held),
            Holding::Never => None,
        }
    }

    /// Keeps `value` under `ticket`, a ticket no other resource has had, and answers with the handle to it.
    fn keep<R: Resource>(&mut self, ticket: u64, value: R) -> Handle<R> {
        self.holdings().push(Keep::Mine(Kept::new(ticket, value)));

        Handle::new(ticket)
    }

    /// Keeps `kept`, a resource given to the actor, as its own, under the ticket it came with.
    pub fn adopt(&mut self, kept: Kept) {
        self.holdings().push(Keep::Mine(kept));
    }

    /// The resource `handle` names.
    fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        self.held()
            .into_iter()
            .flatten()
            .find_map(|keep| match keep {
                Keep::Mine(kept) if kept.ticket == handle.ticket => kept.value.downcast_mut::<R>(),
                _ => None,
            })
            .ok_or_else(Fault::bare::<NotHeld>)
    }

    /// Lets go of the resource of its own `ticket` names, when `fits` it, and answers with it.
    fn give_up(&mut self, ticket: u64, fits: impl Fn(&Kept) -> bool) -> Option<Kept> {
        let held = self.held()?;
        let index =
            held.iter().position(|keep| matches!(keep, Keep::Mine(kept) if kept.ticket == ticket && fits(kept)))?;
        let Keep::Mine(kept) = held.remove(index) else {
            unreachable!("the position found is of a resource the actor owns");
        };

        Some(kept)
    }

    /// Takes the resource `ticket` names, which arrived from the actor `by` with the mail being handled, as the
    /// actor's own, when `fits` it.
    fn take_arrived(&mut self, ticket: u64, by: Seated, fits: impl Fn(&Kept) -> bool) -> bool {
        let Some(held) = self.held() else {
            return false;
        };
        let found = held.iter().position(
            |keep| matches!(keep, Keep::Arriving { by: from, kept, .. } if *from == by && kept.ticket == ticket && fits(kept)),
        );
        let Some(index) = found else {
            return false;
        };
        if let Keep::Arriving { kept, .. } = held.remove(index) {
            held.push(Keep::Mine(kept));
        }

        true
    }

    /// Lets go of the resource of its own `ticket` names, whatever its type: what a guest gives, as its host holds it
    /// to the claim (ADR-0015).
    pub fn give_up_claimed(&mut self, ticket: u64) -> Option<Kept> {
        self.give_up(ticket, any_type)
    }

    /// Takes the resource `ticket` names, which arrived from the actor `by` with the mail being handled, as the
    /// actor's own, whatever its type: what a guest takes, as its host holds it to the claim (ADR-0015).
    pub fn take_claimed(&mut self, ticket: u64, by: Seated) -> bool {
        self.take_arrived(ticket, by, any_type)
    }

    /// Lets go of the resource `ticket` names, its own or one arriving for it: what a guest's host hands to a child
    /// of other code in its config, before the guest's claims of the turn are read (ADR-0015).
    pub fn give_up_any(&mut self, ticket: u64) -> Option<(Kept, Was)> {
        let held = self.held()?;
        let index = held.iter().position(|keep| match keep {
            Keep::Mine(kept) | Keep::Arriving { kept, .. } => kept.ticket == ticket,
        })?;
        let found = match held.remove(index) {
            Keep::Mine(kept) => (kept, Was::Mine),
            Keep::Arriving { by, mail, kept } => (kept, Was::Arriving { by, mail }),
        };

        Some(found)
    }

    /// Keeps `kept`, given by the actor `by` with the mail `mail`, until the actor handles that mail: for the engine,
    /// as the mail's step folds.
    pub fn arrive(&mut self, by: Seated, mail: MailId, kept: Kept) {
        self.holdings().push(Keep::Arriving { by, mail, kept });
    }

    /// Whether it holds the resource `ticket` names, its own or arriving for it.
    #[must_use]
    pub fn holds(&self, ticket: u64) -> bool {
        match &self.holding {
            Holding::Never => false,
            Holding::Holds(holdings) => holdings.held.iter().any(|keep| keep.ticket() == ticket),
        }
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
        let mut untaken = Vec::new();
        if let Some(held) = self.held() {
            held.retain(|keep| match keep {
                Keep::Arriving { mail: with, kept, .. } if *with == mail => {
                    untaken.push(kept.name);
                    false
                }
                _ => true,
            });
        }

        untaken
    }

    /// Takes out what arrived for the actor with mail it never handled, each with the actor that gave it: for the
    /// engine, as the actor leaves its seat. What it owns stays, and is dropped with it.
    pub fn take_arriving(&mut self) -> Vec<(Seated, Kept)> {
        let mut arriving = Vec::new();
        let Some(held) = self.held() else {
            return arriving;
        };
        let mut index = 0;
        while index < held.len() {
            let is_arriving = matches!(held[index], Keep::Arriving { .. });
            if is_arriving {
                if let Keep::Arriving { by, kept, .. } = held.remove(index) {
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

impl Giver {
    /// Who gave what an actor beneath `above` finds in its config.
    #[must_use]
    pub const fn above(above: Above) -> Self {
        match above {
            Above::Chassis => Self::Chassis,
            Above::Actor(parent, _) | Above::Elsewhere(parent) => Self::Actor(parent),
        }
    }
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

    /// `kept`, given by the actor `by`, and carried nowhere yet.
    #[must_use]
    pub const fn loose(by: Seated, kept: Kept) -> Self {
        Self { by: Giver::Actor(by), on: Carried::Nowhere, kept }
    }

    /// Whether it is the resource `ticket` names, given by `by` and carried nowhere yet.
    pub(crate) fn is_loose(&self, by: Giver, ticket: u64) -> bool {
        let is_giver = self.by == by;
        let is_loose = self.on == Carried::Nowhere;

        is_giver && is_loose && self.kept.ticket == ticket
    }

    /// Its ticket.
    #[must_use]
    pub const fn ticket(&self) -> u64 {
        self.kept.ticket
    }
}

/// Takes out of `gifts` the resource `ticket` names, given by `by` and carried nowhere, when `fits` it: what a `begin`
/// takes from its config.
fn take_loose(gifts: &mut Vec<Gift>, by: Giver, ticket: u64, fits: impl Fn(&Kept) -> bool) -> Option<Kept> {
    let index = gifts.iter().position(|gift| gift.is_loose(by, ticket) && fits(&gift.kept))?;

    Some(gifts.remove(index).kept)
}

/// Takes out of `gifts` the resource `ticket` names, given and carried nowhere, whoever gave it: what a guest's host
/// takes back that it handed a child it then refused (ADR-0015).
pub fn take_loose_ticket(gifts: &mut Vec<Gift>, ticket: u64) -> Option<Kept> {
    let index = gifts.iter().position(|gift| gift.on == Carried::Nowhere && gift.kept.ticket == ticket)?;

    Some(gifts.remove(index).kept)
}

/// Takes out of `gifts` the resource `ticket` names, given by `by` and carried nowhere, whatever its type: what a
/// guest's `begin` takes from its config, as its host holds it to the claim (ADR-0015).
pub fn take_claimed(gifts: &mut Vec<Gift>, by: Giver, ticket: u64) -> Option<Kept> {
    take_loose(gifts, by, ticket, any_type)
}

/// Marks each gift `tickets` name, given by `by` and carried nowhere, as carried to `to` with the mail `mail`: refused,
/// marking none, when one names no such gift.
///
/// # Errors
///
/// [`EncodeError::NotCarried`] when a ticket names no gift `by` gave and nothing carries.
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

/// What an actor holds of resources, lent to code the engine cannot see while it computes the actor's turns ahead:
/// its keeping, and what the turn under way has given and nothing carries yet. A guest's host hands from them what the
/// guest puts in the config of a child of other code, as it begins that child (ADR-0015).
pub struct Holds<'a> {
    pub keeping: &'a mut Keeping,
    pub gifts: &'a mut Vec<Gift>,
}

/// Where a resource handed on out of what an actor holds was, to put it back there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Was {
    /// The actor's own.
    Mine,
    /// Arriving for it from `by`, with the mail `mail`.
    Arriving { by: Seated, mail: MailId },
    /// Given in the turn under way by `by`, and carried nowhere.
    Given(Giver),
}

impl Holds<'_> {
    /// Takes out the resource `ticket` names, to hand on, and where it was: one the actor keeps or that has arrived for
    /// it, or one given in the turn under way that nothing carries.
    #[must_use]
    pub fn hand(&mut self, ticket: u64) -> Option<(Kept, Was)> {
        if let Some(found) = self.keeping.give_up_any(ticket) {
            return Some(found);
        }
        let index = self.gifts.iter().position(|gift| gift.on == Carried::Nowhere && gift.kept.ticket == ticket)?;
        let Gift { by, kept, .. } = self.gifts.remove(index);

        Some((kept, Was::Given(by)))
    }

    /// Puts `kept`, handed on and not taken, back where it `was`.
    pub fn put_back(&mut self, kept: Kept, was: Was) {
        match was {
            Was::Mine => self.keeping.adopt(kept),
            Was::Arriving { by, mail } => self.keeping.arrive(by, mail, kept),
            Was::Given(by) => self.gifts.push(Gift { by, on: Carried::Nowhere, kept }),
        }
    }
}

/// What a turn's calls on resources reach: what the engine keeps for the actor, or, in a guest's code, which holds
/// tickets and no values, the claims it makes of its host (ADR-0010, ADR-0015).
pub enum Keeps<'a> {
    /// What the engine keeps for the actor.
    Here(&'a mut Keeping),
    /// A guest's claims, kept in what its turn asked for, which its host holds it to.
    Claimed,
}

impl Keeps<'_> {
    /// Keeps `value` as the actor's own, under `ticket`: refused in a guest's code, which makes no resource.
    pub(crate) fn keep<R: Resource>(&mut self, ticket: u64, value: R) -> Result<Handle<R>, Fault> {
        match self {
            Self::Here(keeping) => Ok(keeping.keep(ticket, value)),
            Self::Claimed => Err(Fault::bare::<KeepsNothing>()),
        }
    }

    /// The resource `handle` names: refused in a guest's code, which opens nothing.
    pub(crate) fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        match self {
            Self::Here(keeping) => keeping.open(handle),
            Self::Claimed => Err(Fault::bare::<KeepsNothing>()),
        }
    }

    /// Lets go of the resource `handle` names, for the actor `me` to give it, into `gifts`, or claims it as given into
    /// `claims` in a guest's code: answers with the given resource that names it.
    pub(crate) fn give<R: Resource>(
        &mut self,
        me: Seated,
        handle: Handle<R>,
        gifts: &mut Vec<Gift>,
        claims: &mut Vec<Claim>,
    ) -> Result<Given<R>, Fault> {
        let ticket = handle.into_ticket();
        match self {
            Self::Here(keeping) => {
                let kept = keeping.give_up(ticket, Kept::is::<R>).ok_or_else(Fault::bare::<NotHeld>)?;
                gifts.push(Gift::loose(me, kept));
            }
            Self::Claimed => claims.push(Claim { by: me, ticket, what: Claimed::Gave }),
        }

        Ok(Given::new(ticket))
    }

    /// Takes the resource `given` names, which came to `me` from the actor `by` with the mail being handled, or claims
    /// it as taken into `claims` in a guest's code.
    pub(crate) fn take_arrived<R: Resource>(
        &mut self,
        me: Seated,
        given: Given<R>,
        by: Seated,
        claims: &mut Vec<Claim>,
    ) -> Result<Handle<R>, Fault> {
        let ticket = given.into_ticket();
        match self {
            Self::Here(keeping) => {
                let took = keeping.take_arrived(ticket, by, Kept::is::<R>);
                if !took {
                    return Err(Fault::bare::<NotHeld>());
                }
            }
            Self::Claimed => claims.push(Claim { by: me, ticket, what: Claimed::Took }),
        }

        Ok(Handle::new(ticket))
    }

    /// Takes the resource `given` names out of `gifts`, given to `me` by `by` and carried nowhere, as its own: what a
    /// `begin` takes from its config. In a guest's code it is claimed into `claims`, and, when the giver is of the
    /// same code, it is no longer the giver's to carry.
    pub(crate) fn take_config<R: Resource>(
        &mut self,
        me: Seated,
        given: Given<R>,
        by: Giver,
        gifts: &mut Vec<Gift>,
        claims: &mut Vec<Claim>,
    ) -> Result<Handle<R>, Fault> {
        let ticket = given.into_ticket();
        match self {
            Self::Here(keeping) => {
                let kept = take_loose(gifts, by, ticket, Kept::is::<R>).ok_or_else(Fault::bare::<NotHeld>)?;
                keeping.adopt(kept);
            }
            Self::Claimed => claims.push(Claim { by: me, ticket, what: Claimed::TookConfig }),
        }

        Ok(Handle::new(ticket))
    }
}

/// What a guest's code claimed of a resource, which its host holds it to when it hands the turn back (ADR-0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// The actor whose turn, or whose `begin`, claimed it.
    pub by: Seated,
    pub ticket: u64,
    pub what: Claimed,
}

/// What a guest claimed it did with a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimed {
    /// It took what came with the mail its turn handles.
    Took,
    /// It took, in its `begin`, what its config holds.
    TookConfig,
    /// It gave up a resource of its own.
    Gave,
    /// The resource it gave goes with the piece of mail its call sent `send`th, counting from nought.
    Carried { send: u32 },
    /// The resource it gave goes in the config of a child of other code, which its host was handed it for as it began
    /// the child.
    Configured,
}

/// Whether the resource `ticket` names was last claimed in `claims` as given by `by`, and nothing since has carried it
/// or taken it: what a guest may write into mail or a config.
#[must_use]
pub fn is_loose_claim(claims: &[Claim], by: Seated, ticket: u64) -> bool {
    let last = claims.iter().rev().find(|claim| claim.ticket == ticket);

    matches!(last, Some(Claim { what: Claimed::Gave, by: giver, .. }) if *giver == by)
}
