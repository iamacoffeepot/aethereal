//! What the actor crate's macros refuse, each a build that fails where the refusal points (ADR-0058, ADR-0059).
//!
//! The kinds and the actor's type each case uses:
//!
//! ```
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! #[derive(Plain)]
//! #[kind(name = "aether.example.ping")]
//! struct Ping;
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.pong")]
//! struct Pong;
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.bell")]
//! struct Bell;
//!
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A block with no `begin`, at the actor's type: "an actor block has a `begin`, which makes the actor from its config".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A word that is no role word, at the attribute: "`#[tel]` is no role word".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tel]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Two role words on one method, at the second: "a method carries one role word".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     #[request]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A `#[tell]` that answers, at its return type: "a `#[tell]` answers nothing, and returns `Result<(), Fault>`".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.pong")]
//! # struct Pong;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<Pong, Fault> {
//!         Ok(Pong)
//!     }
//! }
//! ```
//!
//! A `#[request]` that answers nothing, at its return type: "a `#[request]` answers its asker".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[request]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! An `#[on]` that answers, at its return type: "an `#[on]` returns `Result<(), Fault>`".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, Tick, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.pong")]
//! # struct Pong;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[on]
//!     fn tick(&mut self, _: Tick) -> Result<Pong, Fault> {
//!         Ok(Pong)
//!     }
//! }
//! ```
//!
//! A handler with no second parameter, at its parameters: "a `#[tell]` takes what it handles as its second parameter".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A second parameter that names no kind, at its type: "a `#[tell]` handles the type of its second parameter, which
//! names a kind".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: &Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! An `#[answer]` handed what is not an answer and its context, at its type: "an `#[answer]` is handed
//! `(Answered<R>, C)`".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.pong")]
//! # struct Pong;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[answer]
//!     fn pong(&mut self, _: Pong) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A `#[refused]` with no handler of its kind, at its kind: "a `#[refused]` of `Pong` pairs with a `#[request]` or
//! `#[tell]` of it in this block".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.pong")]
//! # struct Pong;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//!
//!     #[refused]
//!     fn pong_refused(&mut self, _: Pong) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Two `#[refused]` of one kind, at the second's kind: "`Ping` has a `#[refused]` already in this block".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//!
//!     #[refused]
//!     fn refused(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//!
//!     #[refused]
//!     fn refused_again(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Two handlers of one kind, at the second's kind: "this block handles `Ping` already: a kind has one handler".
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//!
//!     #[tell]
//!     fn ping_again(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Two blocks for one type: each macro sees its own block alone, so the second is refused by the compiler, as its
//! `ChildOf` and `Actor` conflict with the first's.
//!
//! ```compile_fail,E0119
//! # use aethereal_actor::{Fault, actor};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//! }
//!
//! #[actor]
//! impl Bell {
//!     fn begin(bell: Self) -> Result<Self, Fault> {
//!         Ok(bell)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A send to a handler a derived guard guards, from a sender of no case's type, at the send: "`Stranger` may not send
//! this mail: the receiver's guard admits only the senders of its cases".
//!
//! ```compile_fail
//! # use aethereal_actor::{ActorRef, Beginning, Fault, Guard, Turn, actor, path};
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.bell")]
//! # struct Bell;
//! # #[actor]
//! # impl Bell {
//! #     fn begin(bell: Self) -> Result<Self, Fault> {
//! #         Ok(bell)
//! #     }
//! # }
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ringer")]
//! # struct Ringer;
//! # #[actor]
//! # impl Ringer {
//! #     fn begin(ringer: Self) -> Result<Self, Fault> {
//! #         Ok(ringer)
//! #     }
//! # }
//! #[derive(Guard)]
//! enum Visitor {
//!     Bell(ActorRef<Bell>),
//!     Ringer(ActorRef<Ringer>),
//! }
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.door")]
//! struct Door;
//!
//! #[actor]
//! impl Door {
//!     fn begin(door: Self) -> Result<Self, Fault> {
//!         Ok(door)
//!     }
//!
//!     #[tell]
//!     fn ping(&mut self, _: Ping, _: &mut Turn<'_, Self, Visitor>) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.stranger")]
//! struct Stranger;
//!
//! #[actor(depends(Door))]
//! impl Stranger {
//!     fn begin(stranger: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
//!         turn.to(path::<Door>(())).send(&Ping)?;
//!
//!         Ok(stranger)
//!     }
//! }
//! ```
//!
//! A derived guard of a generic enum, at its parameters: "a derived `Guard` names its senders' types, so it takes no
//! parameters".
//!
//! ```compile_fail
//! # use aethereal_actor::{ActorRef, Guard};
//! #[derive(Guard)]
//! enum Either<A: 'static, B: 'static> {
//!     First(ActorRef<A>),
//!     Second(ActorRef<B>),
//! }
//! ```
//!
//! A case of a derived guard that holds two fields, at the case: "each case holds one field".
//!
//! ```compile_fail
//! # use aethereal_actor::{ErasedActorRef, Sent};
//! #[derive(Sent)]
//! enum Pair {
//!     Both(ErasedActorRef, ErasedActorRef),
//! }
//! ```
//!
//! A protocol that lists a kind twice, at the second: "`Ping` is listed already: a protocol lists a kind once".
//!
//! ```compile_fail
//! # use aethereal_actor::protocol;
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! #[protocol(Ping, Ping)]
//! struct Pinging;
//! ```
//!
//! A protocol's kind that names what it answers with another word, at the word: "a kind of a protocol takes
//! `answers = A`".
//!
//! ```compile_fail
//! # use aethereal_actor::protocol;
//! # use aethereal_data::Plain;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.ping")]
//! # struct Ping;
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.pong")]
//! # struct Pong;
//! #[protocol(Ping(answer = Pong))]
//! struct Pinging;
//! ```
