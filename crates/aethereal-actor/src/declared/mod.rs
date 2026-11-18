//! What an actor type declares: what it handles, what it reaches by typed path, and what it spawns (ADR-0013,
//! ADR-0059).
//!
//! An author writes the declaration as types: one [`Handles`] impl for each
//! kind an actor handles, the handled kinds as a tuple on the
//! [`Actor`](crate::Actor), the types it reaches by typed path as its
//! `Depends`, and the types it spawns as its `Spawns`. Each send is checked at its call against what it is sent
//! through, and a watch builds only for an actor whose type handles endings.
//! The engine reads the same declaration as the plain data here, made from
//! those types when the program is built.

mod depends;
mod handles;
mod list;
mod lists;
mod protocols;
mod spawns;
mod state;
#[cfg(test)]
mod tests;

pub use depends::{Depends, DependsOn, Last, RootIs, RootsHold};
pub use handles::{Handled, HandledList, Handles, Others, Reached, Received, Receives, Untaken};
pub use list::{Down, Each, Many, One, Pick, Through, TypeList, Whole};
pub use lists::{At, Lists};
pub use protocols::{CarriedBy, Carries, Conforms, Covers, HandledBy, Kinds, Listed, Protocol};
pub use spawns::Spawns;
pub use state::StateSchema;

use crate::{ChildOf, Placement, Spawner, TypeSegment};
use aethereal_data::{ActorKey, ActorPath, Keyed, KindId, KindName};
use core::marker::PhantomData;

/// What an actor type declares, as the engine reads it: its name, its state's schema, what it reaches by typed path,
/// what it spawns, and what it handles (ADR-0013, ADR-0054, ADR-0059).
#[derive(Debug)]
pub struct Declared {
    name: KindName,
    /// The type's id, made from its name and layout when the program is built: what the engine knows its kind by.
    id: KindId,
    /// The schema of an actor's state, its names included, of the layout `id` is made from: what its state is read by
    /// when it moves onto new code (ADR-0054).
    state: StateSchema,
    /// Whether it has one actor or many, each under its own key.
    keyed: Keyed,
    /// Who may spawn an actor of it, as its parent says (ADR-0022).
    spawner: Spawner,
    /// The last segment of the typed path to each type it depends on: what it reaches by typed path (ADR-0059).
    depends: &'static [&'static TypeSegment],
    /// The kind of each type it spawns: what a guest's host checks each spawn its turn makes against (ADR-0059).
    spawns: &'static [KindId],
    /// Each thing it handles.
    handlers: &'static [Handler],
    /// Which kinds a reference to it may be cast to.
    takes: Takes,
}

/// Which kinds a type answers a cast for (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Takes {
    /// Those it lists.
    Listed,
    /// Every kind: the outside, whose code takes whatever it is handed (ADR-0036). A guest's section has no way to say
    /// it, so no guest does.
    Every,
}

impl Declared {
    /// The declaration of the type named `name`, of id `id`, whose state is of the schema `state`, keyed as `keyed`,
    /// that `spawner` may spawn, which depends on the types whose typed paths end in `depends`, spawns the types of
    /// the kinds `spawns`, and handles `handlers`.
    #[must_use]
    #[allow(clippy::too_many_arguments, reason = "each is one part of what a type declares, as the engine reads it")]
    pub const fn new(
        name: KindName,
        id: KindId,
        state: StateSchema,
        keyed: Keyed,
        spawner: Spawner,
        depends: &'static [&'static TypeSegment],
        spawns: &'static [KindId],
        handlers: &'static [Handler],
    ) -> Self {
        Self { name, id, state, keyed, spawner, depends, spawns, handlers, takes: Takes::Listed }
    }

    /// The declaration of the root type named `name`, of id `id`, whose state is of the schema `state`, that handles
    /// nothing and depends on nothing, and that every kind may be cast to: for the engine's outside (ADR-0036).
    #[must_use]
    pub const fn taking_every_kind(name: KindName, id: KindId, state: StateSchema) -> Self {
        let (keyed, spawner, takes) = (Keyed::Singleton, Spawner::Nobody, Takes::Every);

        Self { name, id, state, keyed, spawner, depends: &[], spawns: &[], handlers: &[], takes }
    }

    /// The declaration of the actor type `A`, made when the program is built.
    #[must_use]
    pub const fn of<A: ChildOf>() -> &'static Self {
        Of::<A>::DECLARED
    }

    /// The type's name: the kind of every actor of it, in its path.
    #[must_use]
    pub const fn name(&self) -> &KindName {
        &self.name
    }

    /// The type's id: the kind of every actor of it, as the engine knows it.
    #[must_use]
    pub const fn id(&self) -> KindId {
        self.id
    }

    /// The schema of an actor's state, its names included: what its state is read by when it moves onto new code
    /// (ADR-0054).
    #[must_use]
    pub const fn state(&self) -> StateSchema {
        self.state
    }

    /// Whether it has one actor or many, each under its own key.
    #[must_use]
    pub const fn keyed(&self) -> Keyed {
        self.keyed
    }

    /// Who may spawn an actor of it.
    #[must_use]
    pub const fn spawner(&self) -> Spawner {
        self.spawner
    }

    /// Whether an actor of the type of id `by` may spawn an actor of it, as its parent says (ADR-0022).
    #[must_use]
    pub fn admits(&self, by: KindId) -> bool {
        match self.spawner {
            Spawner::Nobody => false,
            Spawner::Anyone => true,
            Spawner::Own => by == self.id,
            Spawner::Type(id) => by == id,
        }
    }

    /// The last segment of the typed path to each type it depends on: what it reaches by typed path (ADR-0059).
    #[must_use]
    pub const fn depends(&self) -> &'static [&'static TypeSegment] {
        self.depends
    }

    /// The kind of each type it spawns (ADR-0059).
    #[must_use]
    pub const fn spawns(&self) -> &'static [KindId] {
        self.spawns
    }

    /// Whether it lists the type of kind `kind` among what it spawns: what a guest's host checks a spawn its turn
    /// makes by, as a native spawn is checked where it is built (ADR-0059).
    #[must_use]
    pub fn spawns_kind(&self, kind: KindId) -> bool {
        self.spawns.contains(&kind)
    }

    /// Whether the actor at `path`, whose root is the first of its segments, stands beneath a root of a type it
    /// depends on: what a typed path it reaches must (ADR-0059).
    #[must_use]
    pub fn depends_beneath(&self, path: &ActorPath) -> bool {
        let root = path.root();

        self.depends.iter().any(|segment| segment.root().name == root.kind())
    }

    /// Each thing it handles.
    #[must_use]
    pub const fn handlers(&self) -> &'static [Handler] {
        self.handlers
    }

    /// Whether the type lists `kind` among the kinds it handles, or takes every kind. A fallback does not count.
    #[must_use]
    pub fn lists(&self, kind: KindId) -> bool {
        self.takes == Takes::Every || self.handles(Role::Kind(kind))
    }

    /// Whether the type lists every one of `kinds`: the kinds of a protocol it takes (ADR-0016).
    #[must_use]
    pub fn lists_all(&self, kinds: &[KindId]) -> bool {
        kinds.iter().all(|kind| self.lists(*kind))
    }

    /// Whether the type takes mail of `kind`: it lists the kind, or it falls back.
    #[must_use]
    pub fn takes(&self, kind: KindId) -> bool {
        self.lists(kind) || self.handles(Role::Others)
    }

    /// Whether the type handles the ending of an actor it watches.
    #[must_use]
    pub fn handles_endings(&self) -> bool {
        self.handles(Role::Ended)
    }

    /// Whether the type handles what became of a request it asks that is answered with a kind `answer` names
    /// (ADR-0017).
    #[must_use]
    pub fn handles_answer(&self, answer: KindId) -> bool {
        self.handles(Role::Answer(answer))
    }

    /// Whether the type handles the stage of `stage`, as [`On`](crate::On) it: what it may subscribe to (ADR-0020).
    #[must_use]
    pub fn stages(&self, stage: KindId) -> bool {
        self.handles(Role::Stage(stage))
    }

    /// The first stage of the program's lifecycle that the type handles, and so may subscribe to, that `runs` does
    /// not hold, when there is one: what an actor of it is refused for where it is made (ADR-0020).
    pub fn unstaged(&self, runs: impl Fn(KindId) -> bool) -> Option<KindId> {
        self.handlers.iter().find_map(|handler| match handler.role {
            Role::Stage(stage) if !runs(stage) => Some(stage),
            _ => None,
        })
    }

    /// The first typed path of singletons the type depends on whose place `live` does not hold, handed its path and
    /// the kind the actor there must be, when there is one: what an actor of it is refused for where it is made
    /// (ADR-0022, ADR-0059). A path with a key is a place actors fill and empty as the program runs, and is not checked.
    pub fn missing(&self, live: impl Fn(&ActorPath, KindId) -> bool) -> Option<&'static TypeSegment> {
        self.depends.iter().copied().find(|segment| {
            let Some(path) = segment.singletons() else {
                return false;
            };

            !live(&path, segment.kind)
        })
    }

    /// Whether one of the type's handlers is for `role`.
    fn handles(&self, role: Role) -> bool {
        self.handlers.iter().any(|handler| handler.role == role)
    }
}

impl PartialEq for Declared {
    /// Whether the two declare the same: all but the names in their states' schemas, which a spawn does not read, as
    /// each id holds its state's layout (ADR-0006).
    fn eq(&self, other: &Self) -> bool {
        let Self { name, id, state: _, keyed, spawner, depends, spawns, handlers, takes } = self;

        *name == other.name
            && *id == other.id
            && *keyed == other.keyed
            && *spawner == other.spawner
            && *depends == other.depends
            && *spawns == other.spawns
            && *handlers == other.handlers
            && *takes == other.takes
    }
}

impl Eq for Declared {}

/// The declaration of `A`, as a constant.
struct Of<A>(PhantomData<A>);

impl<A: ChildOf> Of<A> {
    const DECLARED: &'static Declared = &Declared::new(
        A::NAME,
        KindId::of::<A>(),
        StateSchema::of::<A>(),
        <A::Key as ActorKey>::KEYED,
        <A::Parent as Placement>::SPAWNER,
        <A::Depends as Depends>::CHAINS,
        <A::Spawns as Spawns>::KINDS,
        <A::Handles as HandledList<A>>::HANDLERS,
    )
    .checked();
}

impl Declared {
    /// It, when what it declares holds together: it depends on no two types of one root, as a typed path beneath it
    /// could be found at either (ADR-0059), it handles each kind of answer with one kind of context (ADR-0017), and
    /// each kind with one handler, as mail is handed to a handler by its kind's id alone: two types of one name and
    /// layout are one kind (ADR-0006).
    ///
    /// # Panics
    ///
    /// Panics, as the type's declaration is made when the program is built, stopping the build, when it does not.
    const fn checked(self) -> Self {
        let mut answers_once = true;
        let mut handles_once = true;
        let mut index = 0;
        while index < self.handlers.len() {
            answers_once = answers_once && !answered_after(self.handlers, index);
            handles_once = handles_once && !handled_after(self.handlers, index);
            index += 1;
        }
        let mut roots_once = true;
        let mut index = 0;
        while index < self.depends.len() {
            roots_once = roots_once && !rooted_after(self.depends, index);
            index += 1;
        }
        assert!(handles_once, "false declaration: a type handles one kind with two handlers");
        assert!(answers_once, "false declaration: a type handles one kind of answer with two kinds of context");
        assert!(roots_once, "false declaration: a type depends on two types of one root");

        self
    }
}

/// Whether the type whose path ends in the segment at `index` of `depends` is of the root of a later one's: two
/// entries a typed path beneath that root could be found at.
const fn rooted_after(depends: &[&TypeSegment], index: usize) -> bool {
    let root = depends[index].root().kind;
    let mut later = index + 1;
    while later < depends.len() {
        if depends[later].root().kind.same(root) {
            return true;
        }
        later += 1;
    }

    false
}

/// Whether the handler at `index` of `handlers` is of an answer a later one handles too: the same answer with two
/// kinds of context, which an actor may not handle (ADR-0017).
const fn answered_after(handlers: &[Handler], index: usize) -> bool {
    let Role::Answer(kind) = handlers[index].role else {
        return false;
    };
    let mut later = index + 1;
    while later < handlers.len() {
        if let Role::Answer(other) = handlers[later].role
            && other.same(kind)
        {
            return true;
        }
        later += 1;
    }

    false
}

/// Whether the handler at `index` of `handlers` is of a kind a later one handles too: two types of one name and layout,
/// whose mail the engine could hand to only one of them.
const fn handled_after(handlers: &[Handler], index: usize) -> bool {
    let Role::Kind(kind) = handlers[index].role else {
        return false;
    };
    let mut later = index + 1;
    while later < handlers.len() {
        if let Role::Kind(other) = handlers[later].role
            && other.same(kind)
        {
            return true;
        }
        later += 1;
    }

    false
}

/// A handler an actor type has: what it handles (ADR-0013).
#[derive(Debug, PartialEq, Eq)]
pub struct Handler {
    role: Role,
}

impl Handler {
    /// The handler for `role`.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self { role }
    }

    /// What it handles.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }
}

/// One thing an actor type handles (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Mail of this kind.
    Kind(KindId),
    /// The ending of an actor it watches.
    Ended,
    /// Mail of any kind it does not list: its fallback.
    Others,
    /// What became of a request it asked whose answer is of this kind (ADR-0017).
    Answer(KindId),
    /// The stage of the program's lifecycle of this kind, which it may subscribe to (ADR-0020).
    Stage(KindId),
}
