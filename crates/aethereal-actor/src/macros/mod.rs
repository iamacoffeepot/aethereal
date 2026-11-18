//! The macros an actor is written with (ADR-0058, ADR-0059), from `aethereal-actor-derive`. What each refuses is held
//! by the doctests of [`refusals`].

mod refusals;

/// An actor written as one block (ADR-0059): `#[actor(…)]` on `impl Name { … }` writes its `ChildOf`, its `Actor`, and
/// one impl for each handler, whose body moves into it as written.
///
/// ```text
/// #[actor(key = K, parent = P, depends(T, …), spawns(C, …), with(library))]
///     key     the actor's `Key`, `()` when left out
///     parent  the type it stands beneath, `Top` when left out
///     depends the types it reaches by typed path; spawns the types it spawns; each empty when left out
///     with    the library whose handlers it takes in: `type Handles = library::handles!(…)`, `library::depends!(…)`
/// ```
///
/// - **`begin`** makes the actor, and the type of its first parameter is the actor's `Config`. **`end`** is its closing
///   turn, and may be left out, ending with `Ok(())`. Either may leave out its `Beginning` or `Ending`.
/// - **Each handler carries one role word**, and its second parameter is what it is handed:
///
///   | Word | Writes | Handed | Returns |
///   |---|---|---|---|
///   | `#[request]` | `Handles<K>` | the kind `K` | `Result<A, Fault>`, `Result<Later<A>, Fault>`, or `Result<Reply<A>, Fault>` |
///   | `#[tell]` | `Handles<K>` | the kind `K` | `Result<(), Fault>` |
///   | `#[answer]` | `Receives<Answer<R, C>>` | `(Answered<R>, C)` | `Result<(), Fault>` |
///   | `#[on]` | `Receives<On<S>>` | the stage's item `S` | `Result<(), Fault>` |
///   | `#[ended]` | `Receives<Ended>` | `Ended` | `Result<(), Fault>` |
///   | `#[others]` | `Receives<Others>` | `Mail<'_>` | `Result<(), Fault>` |
///   | `#[refused]` | `refused` in `Handles<K>` | the kind `K` | what its handler returns |
///
/// - **A handler's guard is its turn's**, `Turn<'_, Self, G>`, written as `type From = G`, and `Anyone` for a turn that
///   names none or a handler that takes no turn; **its answer is its return type**, written as `type Returns`.
/// - **`Handles` is collected from the handlers, in the order written.** `Depends` and `Spawns` are the lists of the
///   attribute: none is `()`, one is that type, and more a tuple; a type parameter of the block counts as the types it
///   lists, `Each<L>`.
/// - **A `#[refused]` is kept as the method written**, and its kind's `refused` calls it, answering what it returns in
///   its handler's form, `Refusal::Answers`. Its turn has no guard.
/// - **Every method of the block has `Spawn` in scope**, so `turn.spawn::<C>(…)` builds with no import of its own.
/// - **A method without a role word**, but `begin` and `end`, stays a method of `impl Name`, as written. The block's
///   generics and where-clause are carried onto every impl it writes.
///
/// One block, with its options and a guarded handler. The pong side stands beneath the ping side, takes a keyed
/// place, and takes `Ping` from a `Pinger` alone; neither writes an `end`, nor the pong side a `Beginning`:
///
/// ```
/// use aethereal_actor::guard::Child;
/// use aethereal_actor::{ActorRef, Beginning, Declared, Fault, Role, Turn, actor};
/// use aethereal_data::{KindId, Plain};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.ping")]
/// struct Ping;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.pong")]
/// struct Pong;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.number")]
/// struct Number;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.pinger")]
/// struct Pinger;
///
/// #[actor(key = u64, spawns(Ponger))]
/// impl Pinger {
///     fn begin(_: Number, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         let Ok(ponger) = turn.spawn::<Ponger>(&0, Number) else {
///             return Err(Fault::new(&Number)?);
///         };
///         turn.to(ponger).send(&Ping)?;
///
///         Ok(Self)
///     }
///
///     #[tell]
///     fn pong(&mut self, _: Pong, turn: &mut Turn<'_, Self, Child<Ponger>>) -> Result<(), Fault> {
///         turn.to(*turn.from()).send(&Ping)?;
///
///         Ok(())
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.ponger")]
/// struct Ponger;
///
/// #[actor(key = u64, parent = Pinger)]
/// impl Ponger {
///     fn begin(_: Number) -> Result<Self, Fault> {
///         Ok(Self)
///     }
///
///     #[tell]
///     fn ping(&mut self, _: Ping, turn: &mut Turn<'_, Self, ActorRef<Pinger>>) -> Result<(), Fault> {
///         turn.to(*turn.from()).send(&Pong)?;
///
///         Ok(())
///     }
/// }
///
/// assert_eq!(Declared::of::<Ponger>().spawns(), &[] as &[KindId]);
/// assert_eq!(Declared::of::<Pinger>().spawns(), &[KindId::of::<Ponger>()]);
/// ```
///
/// Every role word, with what each handles listed in the order written. The board reaches the store by its typed path,
/// as it depends on it; it answers `WhichTile` now, `Fetch` later through its hold, and `Save` either way, takes `Clear`
/// with no turn, and is brought the store's answer, the tick, an ending, and mail of kinds it lists no handler for:
///
/// ```
/// use aethereal_actor::{
///     Answered, Beginning, Declared, Ended, Ending, Fault, Held, Later, Level, Mail, Reply, Role, Tick, Turn, actor,
///     path,
/// };
/// use aethereal_data::{Bound, KindId, Plain};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.which_tile")]
/// struct WhichTile;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.tile_is")]
/// struct TileIs;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.fetch")]
/// struct Fetch;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.fetched")]
/// struct Fetched;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.save")]
/// struct Save;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.saved")]
/// struct Saved;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.clear")]
/// struct Clear;
///
/// /// What the board keeps for a fetch it asks the store.
/// #[derive(Plain)]
/// #[kind(name = "aether.example.fetching")]
/// struct Fetching;
///
/// /// The store: a root that answers a fetch now.
/// #[derive(Plain)]
/// #[kind(name = "aether.example.store")]
/// struct Store;
///
/// #[actor]
/// impl Store {
///     fn begin(store: Self) -> Result<Self, Fault> {
///         Ok(store)
///     }
///
///     #[request]
///     fn fetch(&mut self, _: Fetch) -> Result<Fetched, Fault> {
///         Ok(Fetched)
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.board_config")]
/// struct BoardConfig;
///
/// #[derive(Bound)]
/// #[kind(name = "aether.example.board")]
/// struct Board {
///     tiles: u32,
///     fetching: Vec<Held<Fetched>>,
///     saving: Vec<Held<Saved>>,
/// }
///
/// #[actor(depends(Store))]
/// impl Board {
///     fn begin(_: BoardConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         turn.subscribe::<Tick>();
///
///         Ok(Self { tiles: 0, fetching: Vec::new(), saving: Vec::new() })
///     }
///
///     #[request]
///     fn which(&mut self, _: WhichTile) -> Result<TileIs, Fault> {
///         Ok(TileIs)
///     }
///
///     #[request]
///     fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Fetched>, Fault> {
///         let (held, later) = turn.hold();
///         self.fetching.push(held);
///         turn.to(path::<Store>(())).ask(&fetch, &Fetching)?;
///
///         Ok(later)
///     }
///
///     #[request]
///     fn save(&mut self, _: Save, turn: &mut Turn<'_, Self>) -> Result<Reply<Saved>, Fault> {
///         if self.tiles == 0 {
///             return Ok(Reply::Now(Saved));
///         }
///         let (held, later) = turn.hold();
///         self.saving.push(held);
///
///         Ok(Reply::Later(later))
///     }
///
///     #[tell]
///     fn clear(&mut self, _: Clear) -> Result<(), Fault> {
///         self.tiles = 0;
///
///         Ok(())
///     }
///
///     #[answer]
///     fn fetched(
///         &mut self,
///         (answered, Fetching): (Answered<Fetched>, Fetching),
///         turn: &mut Turn<'_, Self>,
///     ) -> Result<(), Fault> {
///         if let (Answered::Yes(fetched), Some(held)) = (answered, self.fetching.pop()) {
///             turn.answer(held, &fetched)?;
///         }
///
///         Ok(())
///     }
///
///     #[on]
///     fn tick(&mut self, _: Tick) -> Result<(), Fault> {
///         Ok(())
///     }
///
///     #[ended]
///     fn ended(&mut self, _: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         turn.log(Level::Info, "a watched actor ended");
///
///         Ok(())
///     }
///
///     #[others]
///     fn others(&mut self, mail: Mail<'_>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         turn.log(Level::Info, format_args!("passed over {:?}", mail.kind()));
///
///         Ok(())
///     }
///
///     fn end(self, turn: &mut Ending<'_, Self>) -> Result<(), Fault> {
///         turn.log(Level::Info, format_args!("ended with {} tiles", self.tiles));
///
///         Ok(())
///     }
/// }
///
/// let roles: Vec<Role> = Declared::of::<Board>().handlers().iter().map(|handler| handler.role()).collect();
/// let kind = |id| Role::Kind(id);
/// assert_eq!(
///     roles,
///     [
///         kind(KindId::of::<WhichTile>()),
///         kind(KindId::of::<Fetch>()),
///         kind(KindId::of::<Save>()),
///         kind(KindId::of::<Clear>()),
///         Role::Answer(KindId::of::<Fetched>()),
///         Role::Stage(KindId::of::<Tick>()),
///         Role::Ended,
///         Role::Others,
///     ],
/// );
/// ```
///
/// A refusal of its own. Only a keeper may fetch from the vault; anyone else is answered that it is not theirs:
///
/// ```
/// use aethereal_actor::{ActorRef, Fault, Turn, actor};
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.fetch")]
/// struct Fetch;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.fetched")]
/// enum Fetched {
///     Picture,
///     NotYours,
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.keeper")]
/// struct Keeper;
///
/// #[actor]
/// impl Keeper {
///     fn begin(keeper: Self) -> Result<Self, Fault> {
///         Ok(keeper)
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.vault")]
/// struct Vault;
///
/// #[actor]
/// impl Vault {
///     fn begin(vault: Self) -> Result<Self, Fault> {
///         Ok(vault)
///     }
///
///     #[request]
///     fn fetch(&mut self, _: Fetch, _: &mut Turn<'_, Self, ActorRef<Keeper>>) -> Result<Fetched, Fault> {
///         Ok(Fetched::Picture)
///     }
///
///     #[refused]
///     fn not_yours(&mut self, _: Fetch) -> Result<Fetched, Fault> {
///         Ok(Fetched::NotYours)
///     }
/// }
/// ```
///
/// A generic block. Its generics and where-clause are carried onto each impl, and a list type parameter in
/// `depends` or `spawns` counts as what it lists, here `Each<L>` and `Each<R>`:
///
/// ```
/// use aethereal_actor::{Declared, Depends, Fault, Spawns, actor};
/// # use aethereal_data::{
/// #     Boundary, Decode, DecodeError, Encode, EncodeError, Kind, KindId, KindName, Plain, Reader, Schema, Shape, Writer,
/// # };
/// use core::marker::PhantomData;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.keeper_config")]
/// struct KeeperConfig;
///
/// /// A keeper of the roots `R`, which it depends on, and of the watchers `L`, which it spawns.
/// struct Keeper<L, R>(PhantomData<fn() -> (L, R)>);
/// #
/// # impl<L, R> Schema for Keeper<L, R> {
/// #     const SHAPE: Shape = Shape::Struct { name: "Keeper", fields: &[] };
/// # }
/// #
/// # impl<L, R> Kind for Keeper<L, R> {
/// #     const NAME: KindName = KindName::new("aether.example.keeper");
/// # }
/// #
/// # impl<L, R, B: Boundary> Encode<B> for Keeper<L, R> {
/// #     fn encode(&self, _: &mut Writer<'_, B>) -> Result<(), EncodeError> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl<L, R, B: Boundary> Decode<B> for Keeper<L, R> {
/// #     fn decode(_: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
/// #         Ok(Self(PhantomData))
/// #     }
/// # }
///
/// #[actor(depends(R), spawns(L))]
/// impl<L: Spawns + 'static, R: 'static> Keeper<L, R>
/// where
///     R: Depends,
/// {
///     fn begin(_: KeeperConfig) -> Result<Self, Fault> {
///         Ok(Self(PhantomData))
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.watcher")]
/// struct Watcher;
///
/// #[actor(parent = Keeper<(Watcher,), Watched>)]
/// impl Watcher {
///     fn begin(watcher: Self) -> Result<Self, Fault> {
///         Ok(watcher)
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.watched")]
/// struct Watched;
///
/// #[actor]
/// impl Watched {
///     fn begin(watched: Self) -> Result<Self, Fault> {
///         Ok(watched)
///     }
/// }
///
/// let keeper = Declared::of::<Keeper<(Watcher,), Watched>>();
/// assert_eq!(keeper.spawns(), &[KindId::of::<Watcher>()]);
/// assert_eq!(keeper.depends().len(), 1);
/// ```
///
/// A screen takes in its library's handlers through `with(aethereal_ui::screen)`, as `aethereal_ui`'s own example shows.
pub use aethereal_actor_derive::actor;

/// A named protocol (ADR-0016, ADR-0059): `#[protocol(…)]` on a struct writes its `Protocol` and one `Carries` for
/// each kind it lists, in order. A bare kind is told, and `Kind(answers = A)` is asked and answered with `A`. The
/// struct's generics are carried onto each impl, so a protocol's bound is written on the struct.
///
/// ```
/// use aethereal_actor::{Actor, Conforms, Enough, Fault, More, Open, Stream, actor, protocol};
/// use aethereal_data::Plain;
/// use core::marker::PhantomData;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.exited")]
/// struct Exited;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.gather")]
/// struct Gather;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.gathered")]
/// struct Gathered;
///
/// #[protocol(Exited)]
/// struct Exits; // tells only
///
/// #[protocol(Gather(answers = Gathered), Exited)]
/// struct Gathering; // a request and a tell
///
/// #[protocol(Open<S>(answers = S::Done), More, Enough)]
/// struct Streamed<S: Stream>(PhantomData<fn() -> S>); // generic, its bound on the struct
///
/// /// A gatherer, which answers `Gather` as `Gathering` carries it, and so conforms to it.
/// #[derive(Plain)]
/// #[kind(name = "aether.example.gatherer")]
/// struct Gatherer;
///
/// #[actor]
/// impl Gatherer {
///     fn begin(gatherer: Self) -> Result<Self, Fault> {
///         Ok(gatherer)
///     }
///
///     #[request]
///     fn gather(&mut self, _: Gather) -> Result<Gathered, Fault> {
///         Ok(Gathered)
///     }
///
///     #[tell]
///     fn exited(&mut self, _: Exited) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn conforms<T: Actor + Conforms<P>, P>() {}
///
/// conforms::<Gatherer, Gathering>();
/// conforms::<Gatherer, Exits>();
/// ```
pub use aethereal_actor_derive::protocol;

/// A guard of senders of several types (ADR-0058): `#[derive(Guard)]` on a named enum, each case holding one `Sent`
/// type, writes the guard for every actor, its check for every kind, its `Sent`, and its `SentBy`. The guard tries the
/// cases in the order written and hands the handler the first that takes the sender, which it matches over. A send to
/// a handler it guards builds from a sender of any case's type, and from no other.
///
/// ```
/// use aethereal_actor::{ActorRef, Beginning, Fault, Guard, Turn, actor, path};
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.request")]
/// struct Request;
///
/// /// Who may send the server a request: an exchange, or a client.
/// #[derive(Guard)]
/// enum Peer {
///     Exchange(ActorRef<Exchange>),
///     Client(ActorRef<Client>),
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.server")]
/// struct Server {
///     exchanged: u32,
///     served: u32,
/// }
///
/// #[actor]
/// impl Server {
///     fn begin(server: Self) -> Result<Self, Fault> {
///         Ok(server)
///     }
///
///     #[tell]
///     fn request(&mut self, _: Request, turn: &mut Turn<'_, Self, Peer>) -> Result<(), Fault> {
///         match turn.from() {
///             Peer::Exchange(_) => self.exchanged += 1,
///             Peer::Client(_) => self.served += 1,
///         }
///
///         Ok(())
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.exchange")]
/// struct Exchange;
///
/// #[actor(depends(Server))]
/// impl Exchange {
///     fn begin(exchange: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         turn.to(path::<Server>(())).send(&Request)?;
///
///         Ok(exchange)
///     }
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.client")]
/// struct Client;
///
/// #[actor(depends(Server))]
/// impl Client {
///     fn begin(client: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
///         turn.to(path::<Server>(())).send(&Request)?;
///
///         Ok(client)
///     }
/// }
/// ```
///
/// It refuses a generic enum, and a case that does not hold exactly one field. A sender that two cases admit, such as an
/// `ActorRef<T>` case beside an `ErasedActorRef` case, is found at two places, so its send does not build: the place
/// cannot be inferred. Delivery takes it as the case written first.
pub use aethereal_actor_derive::Guard;

/// A checked sender of several types (ADR-0058): `#[derive(Sent)]` writes what `#[derive(Guard)]` writes but the guard,
/// its `Sent` and `SentBy`, for an enum a guard of a handler's own hands on, which writes its own `Guard` and
/// `Checks`. It refuses what `Guard` refuses.
pub use aethereal_actor_derive::Sent;
