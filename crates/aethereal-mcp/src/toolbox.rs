//! The root that stands up the tools a program lists (ADR-0037).

use crate::{Mcp, Offer, Offered, Tool, ToolList, ToolSheet, Withdraw};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, Path, Tells, Top, Turn, path,
};
use aethereal_data::{
    Boundary, Decode, DecodeError, Encode, EncodeError, Field, Kind, KindName, Plain, Reader, Schema, Shape, Writer,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use core::marker::PhantomData;
use std::collections::BTreeSet;

/// What a toolbox is made from: nothing, as its list is its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.toolbox_config")]
pub struct ToolboxConfig;

/// What a toolbox keeps for an offer: nothing, as any refusal ends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.offering")]
pub struct Offering;

/// Why a toolbox failed: a list that names a tool twice, a tool that did not begin, or an offer refused.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.unlisted")]
pub enum Unlisted {
    /// Two tools of the list share this name.
    Repeated(String),
    /// A tool of this kind was refused as it began.
    Unspawned(String),
    /// `Mcp` refused an offer, or did not answer it.
    Refused,
}

/// A program's tools: spawns each of `L` beneath itself and offers it to [`Mcp`] (ADR-0037). It is sugar over
/// [`Offer`], which any actor may ask.
///
/// A list that is not `Mcp`'s to take fails in the order a list's failures can be found: a program with no `Mcp`
/// listed before this does not build, a repeated name or a tool that cannot begin stops the program as it starts, and
/// a name something else offered first ends the toolbox, which withdraws all it offered, the program running on with
/// the fault in the log. Its tools outlive it, as any child does its parent, holding nothing and reached by nothing.
///
/// A program has one, as `Toolbox<L>` has one name whatever `L` is.
///
/// It is listed after `Mcp`, and what `Mcp` needs after that:
///
/// ```
/// # use aethereal_actor::Ordered;
/// # use aethereal_http::Http;
/// # use aethereal_mcp::{Mcp, Toolbox};
/// # use aethereal_tcp::Tcp;
/// # fn ordered<R: Ordered<I>, I>() {}
/// ordered::<(Toolbox<()>, (Mcp, (Http, (Tcp, ())))), _>();
/// ```
///
/// Before it, or with no `Mcp`, the program does not build:
///
/// ```compile_fail
/// # use aethereal_actor::Ordered;
/// # use aethereal_http::Http;
/// # use aethereal_mcp::{Mcp, Toolbox};
/// # use aethereal_tcp::Tcp;
/// # fn ordered<R: Ordered<I>, I>() {}
/// ordered::<(Mcp, (Toolbox<()>, (Http, (Tcp, ())))), _>();
/// ```
pub struct Toolbox<L> {
    /// What to take back as it ends: each name it offered, with the worker that offered it.
    offered: Vec<Withdraw>,
    list: PhantomData<fn() -> L>,
}

impl<L> Schema for Toolbox<L> {
    const SHAPE: Shape =
        Shape::Struct { name: "Toolbox", fields: &[Field { name: "offered", shape: Vec::<Withdraw>::SHAPE }] };
}

/// Named as one, whatever its list.
impl<L> Kind for Toolbox<L> {
    const NAME: KindName = KindName::new("aether.mcp.toolbox");
}

impl<L, B: Boundary> Encode<B> for Toolbox<L>
where
    Vec<Withdraw>: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.offered.encode(out)
    }
}

impl<L, B: Boundary> Decode<B> for Toolbox<L>
where
    Vec<Withdraw>: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Ok(Self { offered: Vec::decode(input)?, list: PhantomData })
    }
}

impl<L: ToolList> ChildOf for Toolbox<L> {
    type Parent = Top;
}

impl<L: ToolList> Actor for Toolbox<L> {
    type Config = ToolboxConfig;
    type Key = ();
    type Handles = (Answer<Offered, Offering>,);
    type BeginSends = (Asks<Path<Mcp>, Offer, Offering>,);
    type EndSends = (Tells<Path<Mcp>, Withdraw>,);

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
            turn.to(path::<Mcp>(())).ask(&offer, &Offering)?;
        }

        Ok(Self { offered, list: PhantomData })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        for withdraw in &self.offered {
            turn.to(path::<Mcp>(())).send(withdraw)?;
        }

        Ok(())
    }
}

impl<L: ToolList> Handles<Answer<Offered, Offering>> for Toolbox<L> {
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
impl<H: Host, L: ToolList> Bootstrap<H> for Toolbox<L> {
    const SECTION: &'static str = "toolbox";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), ToolboxConfig)
    }
}

/// One tool of a list: spawned, and offered.
fn stood_up<T: Tool, L: ToolList, S>(turn: &mut Beginning<'_, Toolbox<L>, S>) -> Result<Offer, Fault> {
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
            fn stand_up<L: ToolList, S>(turn: &mut Beginning<'_, Toolbox<L>, S>) -> Result<Vec<Offer>, Fault> {
                Ok(vec![$(stood_up::<$name, L, S>(turn)?),*])
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
