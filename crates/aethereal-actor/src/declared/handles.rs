//! What an actor handles: one handler for each thing it lists, and the engine's call to the right one (ADR-0013).

use super::{Declared, Handler, Role};
use crate::answering::{Answering, Refusal, Token};
use crate::guard::{Anyone, Checks};
use crate::{
    Actor, Actors, Answer, Ended, ErasedActorRef, Fault, Item, Mail, On, Stage, Turn, Turning, Unreadable, Within,
};
use aethereal_data::{Decode, DecodeError, Kind, KindId};

/// The actor handles mail of the kind `K`, from whoever its guard admits (ADR-0013, ADR-0058).
///
/// `From` is its guard: who may send it `K`, asked before the handler runs. What its guard refuses goes to
/// [`refused`](Self::refused). What the handler sends is checked at each send, against what it sends through.
///
/// `Returns` is how it answers, and the actor's answer to `K`, its [`Carries`](crate::Carries): `()` for a tell, the
/// answer `A` sent now, a [`Later<A>`](crate::Later) its hold made, or a [`Reply<A>`](crate::Reply), either
/// (ADR-0059). The engine sends what it answers now once it returns `Ok`, and the turn has no other way to answer.
/// An answer to mail that asked for none, a request's kind told, is dropped.
///
/// A handler that returns another kind than its `Returns` names does not build:
///
/// ```compile_fail,E0053
/// # use aethereal_actor::{Actor, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Top, Turn};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.which_tile")]
/// # struct WhichTile;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.tile_is")]
/// # struct TileIs;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.no_tile")]
/// # struct NoTile;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.board")]
/// # struct Board;
/// #
/// # impl ChildOf for Board {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Board {
/// #     type Config = Board;
/// #     type Key = ();
/// #     type Handles = (WhichTile,);
/// #     type Depends = ();
/// #     type Spawns = ();
/// #
/// #     fn begin(board: Board, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #         Ok(board)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// impl Handles<WhichTile> for Board {
///     type From = Anyone;
///     type Returns = TileIs;
///
///     fn handle(&mut self, _: WhichTile, _: &mut Turn<'_, Self>) -> Result<NoTile, Fault> {
///         Ok(NoTile)
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no handler of `{K}`",
    label = "an actor of type `{Self}` is to handle `{K}` here",
    note = "a send builds to a type that lists `{K}` in its `Handles` and has an `impl Handles<{K}>`, and a guard \
            `Speaks<P>` admits a sender whose type has one for each kind of `P`"
)]
pub trait Handles<K: Kind>: Actor {
    /// Who may send it `K`: a guard, which hands the handler the sender it checked.
    type From: Checks<Self, K>;
    /// How it answers `K`: `()`, `A`, `Later<A>`, or `Reply<A>`.
    type Returns: Answering;

    /// Handles one piece of mail: a turn of its own, which holds the sender its guard checked. It returns its answer,
    /// which the engine sends.
    fn handle(&mut self, mail: K, turn: &mut Turn<'_, Self, Self::From>) -> Result<Self::Returns, Fault>;

    /// Mail of `K` from a sender `From` refused: by default [`Refusal::Logged`], logged, naming the sender, a request
    /// answered [`Unanswered::Unreadable`](crate::Unanswered::Unreadable), and other mail dropped; or an answer in the
    /// handler's form, [`Refusal::Answers`], which the engine sends as it sends the handler's. Its turn holds the
    /// sender as an [`ErasedActorRef`], as the guard checked nothing to hand on.
    fn refused(&mut self, _mail: K, _turn: &mut Turn<'_, Self>) -> Result<Refusal<Self::Returns>, Fault> {
        Ok(Refusal::Logged)
    }
}

/// The actor takes `H`, which is not mail of a kind it lists: an answer, an ending, a stage, or its fallback (ADR-0013,
/// ADR-0058). No sender chooses these, so none has a guard.
pub trait Receives<H: Received>: Actor {
    /// Handles one item: a turn of its own.
    fn handle(&mut self, item: H::Item<'_>, turn: &mut Turn<'_, Self>) -> Result<(), Fault>;
}

/// Mail of any kind its receiver does not list: the receiver's fallback, which is handed the mail as it came
/// (ADR-0013).
pub struct Others;

/// Something an actor can handle: a kind of mail, [`Ended`], [`On`] a stage, an [`Answer`], or [`Others`]
/// (ADR-0013).
pub trait Handled: 'static {
    /// What it is, as the engine reads it.
    const ROLE: Role;
    /// What its handler is handed.
    type Item<'a>;

    /// What the handler is handed for `item`, which is of this role: or why it is handed nothing.
    fn take(item: Item<'_>) -> Result<Self::Item<'_>, Untaken>;
}

/// What an actor takes that is not mail of a kind it lists, and which no sender chooses, so its handler has no guard:
/// an answer, an ending, a stage, or a fallback (ADR-0058).
pub trait Received: Handled {}

impl Received for Ended {}

impl Received for Others {}

impl<K: Stage> Received for On<K> {}

impl<A: Kind + Decode<Actors> + 'static, C: Kind + Decode<Within> + 'static> Received for Answer<A, C> {}

/// What an actor of type `A` handles, reached through the trait for its role: [`Handles`] for mail of a kind,
/// [`Receives`] for the rest (ADR-0058).
pub trait Reached<A: Actor>: Handled {
    /// Hands `item` to the handler of `A` for it, in a turn of an actor whose type declares `declared`: or, when it is
    /// not to be handed, ends the turn as why says (ADR-0017).
    fn hand(actor: &mut A, item: Item<'_>, turning: Turning<'_>, declared: &'static Declared) -> Result<(), Fault>;
}

impl<A: Handles<K>, K: Kind + Decode<Actors> + 'static> Reached<A> for K {
    #[allow(clippy::inline_always, reason = "once for every turn, in place of the call to the handler it wraps")]
    #[inline(always)]
    fn hand(actor: &mut A, item: Item<'_>, turning: Turning<'_>, declared: &'static Declared) -> Result<(), Fault> {
        let Item::Mail(sent) = item else {
            unreachable!("only mail of its kind is handed to a kind's handler");
        };
        let mail = match K::take(item) {
            Ok(mail) => mail,
            Err(untaken) => return turning.untaken(untaken),
        };
        // The guard reads the type the sender sent the mail as, stamped at the send, and nothing the engine holds of
        // the sender now (ADR-0058).
        let sender = turning.sender(declared, sent.sender_type());
        let admitted = <A::From as Checks<A, K>>::admits(actor, &mail, &sender);

        match admitted {
            Some(from) => {
                let mut turn = turning.declaring::<A, A::From>(declared, from);
                A::handle(actor, mail, &mut turn)?.send(&mut turn, Token::new())?;

                turn.settled()
            }
            None => refuse(actor, mail, turning, declared),
        }
    }
}

/// Hands `mail`, which the guard of `A`'s handler of `K` refused, to the handler's `refused`, in a turn that holds the
/// sender as it came. Out of line, as most mail is admitted.
#[cold]
#[inline(never)]
fn refuse<A: Handles<K>, K: Kind>(
    actor: &mut A,
    mail: K,
    turning: Turning<'_>,
    declared: &'static Declared,
) -> Result<(), Fault> {
    let sender = ErasedActorRef::new(turning.source());
    let mut turn = turning.declaring::<A, Anyone>(declared, sender);
    match A::refused(actor, mail, &mut turn)? {
        Refusal::Answers(answer) => answer.send(&mut turn, Token::new())?,
        Refusal::Logged => turn.refuse::<K>(),
    }

    turn.settled()
}

macro_rules! received {
    ($( [$($generics:tt)*] $handled:ty; )*) => {$(
        impl<A: Receives<$handled>, $($generics)*> Reached<A> for $handled {
            #[allow(
                clippy::inline_always,
                reason = "once for every turn, in place of the call to the handler it wraps"
            )]
            #[inline(always)]
            fn hand(
                actor: &mut A,
                item: Item<'_>,
                turning: Turning<'_>,
                declared: &'static Declared,
            ) -> Result<(), Fault> {
                match <$handled as Handled>::take(item) {
                    Ok(taken) => {
                        let sender = ErasedActorRef::new(turning.source());
                        let mut turn = turning.declaring::<A, Anyone>(declared, sender);
                        <A as Receives<$handled>>::handle(actor, taken, &mut turn)?;

                        turn.settled()
                    }
                    Err(untaken) => turning.untaken(untaken),
                }
            }
        }
    )*};
}

received! {
    [] Ended;
    [] Others;
    [K: Stage] On<K>;
    [R: Kind + Decode<Actors> + 'static, C: Kind + Decode<Within> + 'static] Answer<R, C>;
}

/// Why an item was not handed to its handler (ADR-0017).
#[derive(Debug)]
pub enum Untaken {
    /// Its mail did not read as its kind, for this reason: its sender's fault, so the handler does not run and the
    /// turn does not fail. The engine answers a request [`Unanswered::Unreadable`](crate::Unanswered::Unreadable), and
    /// drops other mail, and the receiver's log records why.
    Unread(Unreadable),
    /// What the engine kept for it did not read, which its receiver wrote: the turn fails with this fault.
    Failed(Fault),
}

impl Untaken {
    /// An item whose mail did not read, as `error` says. Out of line, as most mail reads.
    #[cold]
    #[inline(never)]
    pub(crate) fn unread(error: DecodeError) -> Self {
        Self::Unread(error.into())
    }

    /// An item whose kept part did not read, as `error` says. Out of line, as what is kept reads.
    #[cold]
    #[inline(never)]
    pub(crate) fn failed(error: DecodeError) -> Self {
        Self::Failed(error.into())
    }
}

impl<K: Kind + Decode<Actors> + 'static> Handled for K {
    const ROLE: Role = Role::Kind(KindId::of::<K>());
    type Item<'a> = K;

    fn take(item: Item<'_>) -> Result<K, Untaken> {
        let Item::Mail(mail) = item else {
            unreachable!("only mail of its kind is handed to a kind's handler");
        };

        mail.contents().map_err(Untaken::unread)
    }
}

impl Handled for Ended {
    const ROLE: Role = Role::Ended;
    type Item<'a> = Self;

    fn take(item: Item<'_>) -> Result<Self, Untaken> {
        let Item::Ended(ended) = item else {
            unreachable!("only an ending is handed to the handler of endings");
        };

        Ok(ended)
    }
}

impl Handled for Others {
    const ROLE: Role = Role::Others;
    type Item<'a> = Mail<'a>;

    fn take(item: Item<'_>) -> Result<Mail<'_>, Untaken> {
        let Item::Mail(mail) = item else {
            unreachable!("only mail is handed to a fallback");
        };

        Ok(mail)
    }
}

/// Everything an actor of type `A` handles: a tuple of [`Handled`], each with its [`Handles`] or [`Receives`] impl
/// (ADR-0013).
///
/// A tuple holds up to 64: an actor that handles more is several actors.
pub trait HandledList<A: Actor>: 'static {
    /// Each thing handled, as the engine reads them.
    const HANDLERS: &'static [Handler];

    /// Hands `item` to the handler for it: the one for its kind or for endings, else the fallback for mail, in a turn
    /// of an actor whose type declares `declared`.
    ///
    /// # Panics
    ///
    /// Panics when `A` has no handler for `item`: its sender declared a send that `A` does not take (ADR-0013).
    fn dispatch(actor: &mut A, item: Item<'_>, turning: Turning<'_>, declared: &'static Declared) -> Result<(), Fault>;
}

/// What `item` is, as its receiver's handlers are listed.
fn role(item: &Item<'_>) -> Role {
    match item {
        Item::Mail(mail) => Role::Kind(mail.kind()),
        Item::Ended(_) => Role::Ended,
        // An answer is handed to the handler of the kind its request was asked for, whatever kind it came as, which
        // reads one of another kind as unreadable (ADR-0017).
        Item::Answer { asking, .. } | Item::Unanswered { asking, .. } => Role::Answer(asking.answer),
    }
}

/// Whether the handler listed for `listed` takes an item of `role`: its own, or, for a stage's handler, mail of the
/// stage's kind, as the engine sends a stage (ADR-0020).
fn takes(listed: Role, role: Role) -> bool {
    let is_own = listed == role;
    let is_stage = matches!((listed, role), (Role::Stage(stage), Role::Kind(kind)) if stage == kind);

    is_own || is_stage
}

/// Stops the engine: an actor of type `A` was handed an item it has no handler for.
fn unhandled<A: Actor>(role: Role) -> ! {
    panic!("false declaration: {role:?} was sent to an actor of type {}, which does not take it", A::NAME);
}

macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<A: Actor, $($name: Reached<A>),*> HandledList<A> for ($($name,)*) {
            const HANDLERS: &'static [Handler] =
                &[$(Handler::new($name::ROLE)),*];

            fn dispatch(
                actor: &mut A,
                item: Item<'_>,
                turning: Turning<'_>,
                declared: &'static Declared,
            ) -> Result<(), Fault> {
                let role = role(&item);
                $(
                    if takes($name::ROLE, role) {
                        return $name::hand(actor, item, turning, declared);
                    }
                )*
                let is_mail = matches!(item, Item::Mail(_));
                $(
                    let falls_back = $name::ROLE == Role::Others;
                    if is_mail && falls_back {
                        return $name::hand(actor, item, turning, declared);
                    }
                )*

                unhandled::<A>(role)
            }
        }
    )*};
}

tuples! {
    [B]
    [B C]
    [B C D]
    [B C D E]
    [B C D E F]
    [B C D E F G]
    [B C D E F G H]
    [B C D E F G H I]
    [B C D E F G H I J]
    [B C D E F G H I J K]
    [B C D E F G H I J K L]
    [B C D E F G H I J K L M]
    [B C D E F G H I J K L M N]
    [B C D E F G H I J K L M N O]
    [B C D E F G H I J K L M N O P]
    [B C D E F G H I J K L M N O P Q]
    [B C D E F G H I J K L M N O P Q T17]
    [B C D E F G H I J K L M N O P Q T17 T18]
    [B C D E F G H I J K L M N O P Q T17 T18 T19]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62 T63]
    [B C D E F G H I J K L M N O P Q T17 T18 T19 T20 T21 T22 T23 T24 T25 T26 T27 T28 T29 T30 T31 T32 T33 T34 T35 T36 T37 T38 T39 T40 T41 T42 T43 T44 T45 T46 T47 T48 T49 T50 T51 T52 T53 T54 T55 T56 T57 T58 T59 T60 T61 T62 T63 T64]
}

impl<A: Actor> HandledList<A> for () {
    const HANDLERS: &'static [Handler] = &[];

    fn dispatch(_: &mut A, item: Item<'_>, _: Turning<'_>, _: &'static Declared) -> Result<(), Fault> {
        unhandled::<A>(role(&item))
    }
}
