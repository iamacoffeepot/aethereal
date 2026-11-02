//! The root that stands up the tools a program lists, and offers them to its holder (ADR-0037, ADR-0038).

use crate::{Holder, Offer, Offered, Tool, ToolList, ToolSheet, Withdraw};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, Path, RepliesTo, Tells, Top,
    Turn, path,
};
use aethereal_data::{
    Boundary, Decode, DecodeError, Encode, EncodeError, Field, Kind, KindName, Plain, Reader, Schema, Shape, Writer,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use core::marker::PhantomData;
use std::collections::BTreeSet;

/// What a toolbox is made from: nothing, as its list is its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.toolbox_config")]
pub struct ToolboxConfig;

/// What a toolbox keeps for an offer: nothing, as any refusal ends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.offering")]
pub struct Offering;

/// Why a toolbox failed: a list that names a tool twice, a tool that did not begin, or an offer refused.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.unlisted")]
pub enum Unlisted {
    /// Two tools of the list share this name.
    Repeated(String),
    /// A tool of this kind was refused as it began.
    Unspawned(String),
    /// The holder refused an offer, or did not answer it.
    Refused,
}

/// A program's tools: spawns each of `L` beneath itself and offers it to `R`, the root that holds them (ADR-0037,
/// ADR-0038): `Mcp` in a hub, `Link` in an engine. It is sugar over [`Offer`], which any actor may ask.
///
/// A list that is not its holder's to take fails in the order a list's failures can be found: a program with no `R`
/// listed before this does not build, a repeated name or a tool that cannot begin stops the program as it starts, and
/// a name something else offered first ends the toolbox, which withdraws all it offered, the program running on with
/// the fault in the log. Its tools outlive it, as any child does its parent, holding nothing and reached by nothing.
///
/// A program has one, as `Toolbox<L, R>` has one name whatever `L` and `R` are.
pub struct Toolbox<L, R> {
    /// What to take back as it ends: each name it offered, with the worker that offered it.
    offered: Vec<Withdraw>,
    list: PhantomData<fn() -> (L, R)>,
}

impl<L, R> Schema for Toolbox<L, R> {
    const SHAPE: Shape =
        Shape::Struct { name: "Toolbox", fields: &[Field { name: "offered", shape: Vec::<Withdraw>::SHAPE }] };
}

/// Named as one, whatever its list and holder.
impl<L, R> Kind for Toolbox<L, R> {
    const NAME: KindName = KindName::new("aether.tools.toolbox");
}

impl<L, R, B: Boundary> Encode<B> for Toolbox<L, R>
where
    Vec<Withdraw>: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.offered.encode(out)
    }
}

impl<L, R, B: Boundary> Decode<B> for Toolbox<L, R>
where
    Vec<Withdraw>: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Ok(Self { offered: Vec::decode(input)?, list: PhantomData })
    }
}

impl<L: ToolList, R: Holder> ChildOf for Toolbox<L, R>
where
    <R as Handles<Withdraw>>::Sends: RepliesTo<Self>,
{
    type Parent = Top;
}

impl<L: ToolList, R: Holder> Actor for Toolbox<L, R>
where
    <R as Handles<Withdraw>>::Sends: RepliesTo<Self>,
{
    type Config = ToolboxConfig;
    type Key = ();
    type Handles = (Answer<Offered, Offering>,);
    type BeginSends = (Asks<Path<R>, Offer, Offering>,);
    type EndSends = (Tells<Path<R>, Withdraw>,);

    fn begin(_: ToolboxConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut names = BTreeSet::new();
        for sheet in L::sheets() {
            let is_fresh = names.insert(sheet.name.clone());
            if !is_fresh {
                return Err(Fault::new(&Unlisted::Repeated(sheet.name))?);
            }
        }
        let mut offered = Vec::new();
        for offer in L::stand_up(turn)? {
            let worker = offer.worker;
            offered.extend(offer.tools.iter().map(|sheet| Withdraw { tool: sheet.name.clone(), worker }));
            turn.to(path::<R>(())).ask(&offer, &Offering)?;
        }

        Ok(Self { offered, list: PhantomData })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        for withdraw in &self.offered {
            turn.to(path::<R>(())).send(withdraw)?;
        }

        Ok(())
    }
}

impl<L: ToolList, R: Holder> Handles<Answer<Offered, Offering>> for Toolbox<L, R>
where
    <R as Handles<Withdraw>>::Sends: RepliesTo<Self>,
{
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Offering): (Answered<Offered>, Offering),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        if answered == Answered::Yes(Offered::Yes) {
            return Ok(());
        }
        turn.log(Level::Error, format_args!("an offer was refused: {answered:?}"));

        Err(Fault::new(&Unlisted::Refused)?)
    }
}

/// It stands up on every host, its list in its type and no settings of its own.
impl<H: Host, L: ToolList, R: Holder> Bootstrap<H> for Toolbox<L, R>
where
    <R as Handles<Withdraw>>::Sends: RepliesTo<Self>,
{
    const SECTION: &'static str = "toolbox";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), ToolboxConfig)
    }
}

/// One tool of a list: spawned, and offered.
fn stood_up<T: Tool, L: ToolList, R: Holder, S>(turn: &mut Beginning<'_, Toolbox<L, R>, S>) -> Result<Offer, Fault>
where
    <R as Handles<Withdraw>>::Sends: RepliesTo<Toolbox<L, R>>,
{
    let Ok(me) = turn.spawn::<T>(&(), T::Config::default()) else {
        return Err(Fault::new(&Unlisted::Unspawned(T::NAME.to_string()))?);
    };

    Ok(Offer { tools: T::sheets(), worker: T::worker(me) })
}

macro_rules! tools {
    ($( ($($name:ident)*) )*) => {$(
        impl<$($name: Tool),*> ToolList for ($($name,)*) {
            fn sheets() -> Vec<ToolSheet> {
                let sheets: Vec<Vec<ToolSheet>> = vec![$($name::sheets()),*];

                sheets.concat()
            }

            #[allow(unused_variables)] // An empty list spawns nothing.
            fn stand_up<L: ToolList, R: Holder, S>(turn: &mut Beginning<'_, Toolbox<L, R>, S>) -> Result<Vec<Offer>, Fault>
            where
                <R as Handles<Withdraw>>::Sends: RepliesTo<Toolbox<L, R>>,
            {
                Ok(vec![$(stood_up::<$name, L, R, S>(turn)?),*])
            }
        }
    )*};
}

tools! {
    ()
    (A)
    (A B)
    (A B C)
    (A B C D)
    (A B C D E)
    (A B C D E F)
    (A B C D E F G)
    (A B C D E F G H)
}
