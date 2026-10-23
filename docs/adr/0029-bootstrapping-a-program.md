# ADR-0029: Bootstrapping a program

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

A program is started by hand, the same way on every host. `crates/aethereal-chassis-desktop/examples/window.rs` writes it out:
- the engine from five positional literals, `Engine::new(workers, 1024, 1 << 20, 1 << 20, 0)`;
- the cycle, `with_cycle::<(Tick, Render)>()`;
- the window root's handles in pairs, `Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>())`, a window opened on the `Screen`, and `opening.config()` split into the root's config and the host's end, `Hosting`;
- the renderer's config from the GPU on that window, `renderer(&mut setup, screen, hosting)`, and the host's end of it, `Drawing`;
- each root added with `add_root`, in order, and `Made { engine, drawing }` handed back to the host.

Every program on the browser writes the same lines again with `open([canvas], ..).await`, and keeps the canvas's and the controls' listeners alive itself. A program that adds a capability has to know how that capability's host ends are made and where they go.

ADR-0028 makes a program's configuration plain data, read once as the program is set up, and leaves the second half of a standard start open: how each root's config is built from the configuration and from what the host makes. That half decides what a section belongs to. A configuration whose sections are listed apart from the roots that read them can hold a section nothing reads, which is a setting that does nothing, the failure ADR-0028 exists to prevent. And it can lack a section a root reads, found only when the program runs.

What a setup already checks: `Setup::ready` builds only when each root reaches at a typed path of a singleton root only a root made before it (`Ordered<I>`, `MadeAfter`, in `aethereal-actor/src/path/roots.rs`, ADR-0022). A root states whom it reaches in its sends (`BeginSends`, each handler's `Sends`, `EndSends`), never in its config. What a root's config holds beyond plain data is what the host made: a `Given` (ADR-0019), a source or a controller made in a pair whose other end the host keeps (`Setup::source`, `Setup::controller`), and values another root's setup made, such as the `WindowId` a window was opened under.

## Decision

**A program is a host, the engine's options, and a list of roots, in order. Each root bootstraps itself.**

```rust
Desktop::program()
    .with_guest_jobs::<(Sum,)>()
    .with_engine(EngineSettings { workers, ..Default::default() })
    .root::<Windows>()
    .root::<Renderer>()
    .root::<CameraRig>()
    .root_with::<Scene>(Show { seconds: 30 })
    .run()
```

- `.root::<A>()` names an actor. There is no second noun: a root is the actor and how it stands itself up. Its settings start from their kind's `Default`, and only this method asks for one.
- `.root_with::<A>(settings)` lists it the same way, with this program's defaults for its section, a whole typed value: `Show { seconds: 30 }`, or `Show { seconds: 30, ..Default::default() }` when the kind has a `Default`. A root whose settings have no honest default is listed only this way.
- The engine's options keep the words `Setup` already uses: `with_cycle`, `with_guest_jobs`, `with_guests`, `with_links`, `with_delta_limit`. They make no root and are set before any root, as `Setup<()>` requires. Beside them, `with_engine(..)` and `with_running(..)` give the program's defaults for the host's two sections.
- **`with_guest_jobs::<(Sum,)>()` lists the job types the program's guests may run, and the host makes the runners that run every job** (ADR-0018: `Jobs`). A native turn's job carries its own code, so a program that runs no guest lists no job. A program never makes a runner: the host knows its threads and its event loop, and it makes its runners once the program's roots have bootstrapped, with what the roots served it, with `Engine::set_jobs`: natively a pool on the cores the engine's workers leave free (`Runners::beside`), and in a browser the page's event loop (`EventLoop`). A job a root's `begin` runs before then waits, as a job past the most the host lets be in flight does, and is handed out once the runners are made. **This departs from a program handing the engine a runner it made** (`with_jobs(Arc<dyn Jobs>)`), which left the host no runner of its own to add to: a root a host bootstraps can then give the host what its jobs need, as a file capability gives its storage (ADR-0032). `Setup::with_jobs` still takes a runner, for an engine a test makes without a host.
- The defaults these give are the program's, beneath every layer of ADR-0028: the command line, the environment, the file, and the page all win over them.
- `run` is the host's: it bootstraps the program, then runs it, as `Desktop::run` and `Browser` do now. It answers one error, the host's, which holds a program that did not start beside whatever else stops that host running it, so a program reads `program.run()?`.
- **The headless host also has `build`**: both phases, then the host handed back with its engine, not yet run, for a test to `step` or `run_until_idle`. A host whose setup happens inside its event loop, as the desktop's does in winit's first callback, has only `run`.

**How a root bootstraps is a trait on the actor, for a host.**

```rust
pub trait Bootstrap<H: Host>: Root {
    /// The section this root reads its settings from (ADR-0028).
    const SECTION: &'static str;
    /// Its settings: plain data, `()` when it has none. A `Default` is needed only to list it with `.root`.
    type Settings;
    /// What it takes from roots bootstrapped before it.
    type Needs;
    /// What it gives roots bootstrapped after it.
    type Provides;

    fn bootstrap(
        settings: Self::Settings,
        needs: Self::Needs,
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> impl Future<Output = Result<Self::Provides, Unbuilt>>;
}
```

- **A bootstrap is async**, and an impl writes `async fn bootstrap`. One trait serves every host, so `impl<H: Host> Bootstrap<H> for Scene` runs on a host whose setup awaits and on one that does not.
- **The host's crate writes it for the roots that need the host:** `impl Bootstrap<Desktop> for Windows` in `aethereal-chassis-desktop`, `impl Bootstrap<Browser> for Windows` in `aethereal-chassis-browser`. The same list runs on every host that bootstraps each of its roots, and a root a host does not bootstrap does not build there.
- **A library or the program writes it for a root that needs no host:** `impl<H: Host> Bootstrap<H> for Scene`.
- **Only the crate of the actor or of the host can write it.** `Bootstrap`, the host, and the actor would all be foreign to a program's crate, which Rust's orphan rule refuses. So a library ships a bootstrap for each actor it means to be a root, generic over the host where it needs none, and an actor shipped without one cannot be listed.
- **An actor whose config is its settings opts in with `#[derive(Bootstrap)]`.** Its config is plain data, it needs nothing, and it provides nothing; the derive writes `bootstrap` as beginning it from its settings. It is a derive, not a blanket impl, because a blanket impl over every such actor would overlap every impl a host writes.
- **`Bootstrapping<'_, H, A>` begins only `A`**, once for a singleton and once per key for an instanced root, so each `.root::<A>()` adds exactly `A` to the list `Ordered<I>` checks. It makes handles (`hand`, `source`, `controller`), puts code and blobs, and reaches the host's own setup, such as the desktop's `Screen`.
- **A host end goes to the host through `cx.serve(end)`,** which builds only when the host serves that end (`Desktop: Serves<Hosting>`). A pair is made and split inside one bootstrap: its `Given` half goes into the root's config, its other half to `serve`, and neither leaves.

```rust
impl Bootstrap<Desktop> for Windows {
    const SECTION: &'static str = "windows";
    type Settings = WindowSettings;
    type Needs = ();
    type Provides = Opened;

    async fn bootstrap(settings: WindowSettings, (): (), cx: &mut Bootstrapping<'_, Desktop, Self>) -> Result<Opened, Unbuilt> {
        let mut opening = Opening::new(cx.source(), cx.controller());
        // each window in `settings` opened on the screen, then:
        let (config, hosting) = opening.config();
        cx.begin(&(), config)?;
        cx.serve(hosting);
        Ok(opened)
    }
}

impl<H: Host> Bootstrap<H> for Scene {
    const SECTION: &'static str = "show";
    type Settings = Show;
    type Needs = (Opened,);
    type Provides = ();

    async fn bootstrap(show: Show, (opened,): (Opened,), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Shows { window: opened.first(), seconds: show.seconds })
    }
}
```

**Settings come first.** Bootstrapping runs in two phases, and no actor is stood up until the first is done.

1. **Settings.** Each root's `Settings`, under its `SECTION`, and the host's own sections (`engine`, `running`) are the composite of ADR-0028, known from the list's types. Every layer is read and decoded into them before any window opens or any root begins. Two sections of one name are refused here, naming both roots.
2. **Roots.** The engine is made from its section and its options. Then each root bootstraps, in the order listed, handed its settings and what it needs.

A section belongs to the root that reads it, so a composed section that nothing reads, and a read section that was never composed, cannot be written.

**What is checked, and when.**

| | when |
|---|---|
| a root reaches a singleton root not bootstrapped before it | compile, at `run` (`Ordered<I>`, as at `ready` now) |
| a root needs what no root before it provides | compile, at `run` / `build`, naming the type |
| a root the host does not bootstrap | compile, at `.root` |
| a host end the host does not serve | compile, in the root's `bootstrap` |
| two sections of one name | phase 1, before anything opens, naming both roots |
| a flag, variable, file, or page that sets what does not exist or the wrong shape | phase 1, naming the path and the layer (ADR-0028) |
| a window or a GPU that does not open, a root refused as it begins | phase 2, naming the root |

**A bootstrap that fails stops the program as it is set up.** `Unbuilt` says why:
- the root was refused as it began (`Refused`);
- something the host opens did not open: a window, a GPU, a canvas;
- the bootstrap's own failure, such as a module file that could not be read, carried as an error of its own.

`run` and `build` report it naming the root whose bootstrap failed. The engine and the roots begun before it are dropped, as a setup whose `add_root` is refused is dropped now.

**Needs and provides are found by type.** A root's `Needs` and `Provides` are tuples of types; each need is found among what roots before it provided, as `Ordered<I>` finds a root, and handed over as a clone. The check is made at `run` / `build`, not at `.root`: finding a need there takes an index the compiler infers, which a turbofish would make every `.root::<A, _>()` spell out. A type is provided once in a program; two roots that provide one type are refused only where a later root needs it, as an ambiguous type. A root that needs two values of one kind needs two types.

**A missing root is an error, never filled in.** The list cannot add a root a later one reaches: that asks the type system whether a root is already in the list, which stable Rust cannot answer. Which root stands where, on which host, is the program's to say.

**A host that awaits awaits the bootstrap; a host that does not blocks on it.** Opening a canvas's GPU is a future (`open(..).await`), so the browser awaits each root's bootstrap, and its `run` answers a future the page awaits. The desktop and the headless host do not await: each blocks its own thread on the bootstrap, as the desktop already blocks on wgpu's adapter and device. A bootstrap that waits is never refused.

**What a host opens at bootstrap is the host's.** The windows and canvases the window root's bootstrap opens are kept in the host's hosting, never handed off by being taken out of it. A root bootstrapped after it, such as the renderer, reaches them through the host (`cx.host()`), borrowing what it needs to build its config and sharing what it draws with; nothing is taken out of the hosting to mark it used. The host's drawing then reaches the windows as the program runs.

**Only how the program starts.** A window opened while the program runs (ADR-0027), a child, a guest loaded later, a setting changed later: each is mail to a root the list stood up, never a root of the list.

## Consequences

- A program is a list of what it runs. The setup each example writes by hand today, the handle pairs, the host ends, the order of `add_root`, the listeners the browser keeps alive, moves into each root's bootstrap, written once by whoever owns the root on that host.
- A program written as a list runs on each host that bootstraps its roots, unchanged.
- Each section of ADR-0028 has an owner: a root, or the host. A setting no root reads cannot be composed.
- `Desktop`'s fields and `Engine::new`'s positional arguments become the host's sections, `running` and `engine`.
- A root that sends to a protocol (ADR-0016) rather than a type's path can be bootstrapped beside a stand-in of another type, for a test; a root that sends to `Path<Renderer>` needs the `Renderer`.
- One bootstrap per actor per host. A second way to stand up one actor on one host is a second actor type.
- A setting no root reads has no section. What a host reads is the host's section; anything else is a root's.
- **Left for later:**
  - a program renaming a root's section, as it renames a flag; two roots that declare one name are refused until then;
  - the cycle's stages declared by the roots that run in them, rather than by the program in `with_cycle`.

## Alternatives considered

- **A list of sections beside the list of roots**, each section taken by its reader. Two lists kept in step: a type-level check can hold them together, but the program still writes each name twice.
- **Setup steps as their own types**, `OpenWindows` beside the `Windows` actor. A second noun for the same thing, and a program would have to know which step stands up which actor.
- **A closure per root in the program** (`.root::<Scene>(|show: &Show, opened: &Opened| ..)`). Reads well once, but it is a second way to bootstrap beside the trait, and its errors are hard to read.
- **A blanket bootstrap for every actor whose config is plain.** It overlaps every bootstrap a host writes; Rust's coherence does not tell them apart by their bounds.
- **The section named by the root's path segment.** That name is the kind's, `aether.windows`, whose dots are a path's separator in `--set`.
- **Settings read as each root is stood up.** A bad setting would be found after windows were opened and roots begun, and the schema of the program would not be known before it ran.
