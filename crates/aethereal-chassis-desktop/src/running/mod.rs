//! The program run on winit's event loop through its lifecycle: started once its windows and GPU are open, cycled each
//! frame, as the display frees one, as fast as they are made, or at a rate, its windows opened and closed between
//! steps as the window root asks, and stopped when its last window has closed or it quits (ADR-0020, ADR-0021,
//! ADR-0024, ADR-0027).

mod pacing;
mod window;
mod windowing;

use crate::gpu::Screen;
use crate::redrawing::Hold;
use crate::serving::Drawing;
use aethereal_actor::Time;
use aethereal_chassis_headless::{Exit, Headless, Monotonic};
use aethereal_pace::Clockwork;
use aethereal_substrate::{Clock, Lifecycle, ProgramError, Settled};
use aethereal_window::{Hosting, MadeCursors};
use core::error::Error;
use core::fmt;
use core::mem;
use core::num::NonZeroUsize;
use core::time::Duration;
use pacing::{Due, Pacing, Undrawing, Undrawn};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Wake, Waker};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{CustomCursor, WindowId};

/// What the engine sends the event loop when a source is pushed or a job finishes, to wake it (ADR-0010, ADR-0018):
/// it carries nothing, and the loop steps what came in as it next settles.
struct Woken;

/// What the engine wakes the event loop by: its proxy, through which any thread sends it [`Woken`]. A proxy is not
/// shared between threads on every platform, so it is behind a lock, held only to send.
struct Proxy(Mutex<EventLoopProxy<Woken>>);

impl Wake for Proxy {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // A loop that has ended has nothing left to wake.
        let _ = self.0.lock().unwrap_or_else(PoisonError::into_inner).send_event(Woken);
    }
}

/// The desktop host: a program's host by its type alone (ADR-0029). Its program is written
/// `Desktop::program().root::<Windows>().root::<Renderer>()…`, and run with
/// [`run`](aethereal_substrate::Program::run). The engine runs on as many workers as the engine's section says, the
/// time `Stop` has to finish is its own section's, and the windows it opens are the window root's settings'.
#[derive(Debug)]
pub enum Desktop {}

/// Why a program on the desktop did not run to its end: it did not start, or the desktop could not run it.
#[derive(Debug)]
pub enum Failed {
    /// The program did not start, as this says.
    Unstarted(ProgramError),
    /// winit's event loop did not run.
    EventLoop(EventLoopError),
    /// The program bootstrapped without serving the desktop an end it runs a program with, named here: it lists no
    /// renderer, or no window root.
    Unserved(&'static str),
    /// The event loop ended before the program did: before it was bootstrapped, or with it still running.
    Unended,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unstarted(error) => write!(f, "{error}"),
            Self::EventLoop(error) => write!(f, "the event loop did not run: {error}"),
            Self::Unserved(end) => write!(f, "the program served the desktop no end of {end}"),
            Self::Unended => write!(f, "the event loop ended before the program did"),
        }
    }
}

impl Error for Failed {}

impl From<ProgramError> for Failed {
    fn from(error: ProgramError) -> Self {
        Self::Unstarted(error)
    }
}

impl Desktop {
    /// Runs the program `settled` on winit's event loop, as the desktop's [`run`](aethereal_substrate::Host::run) says.
    pub(crate) fn run_settled(settled: Settled<Self>) -> Result<Exit, Failed> {
        let workers = settled.engine().worker_threads();
        let stop_within = Duration::from_millis(settled.running().stop_within_millis);
        let event_loop = EventLoop::<Woken>::with_user_event().build().map_err(Failed::EventLoop)?;
        let proxy = event_loop.create_proxy();
        let mut app =
            App { workers, stop_within, clock: Monotonic::new(), proxy, stage: Stage::Settled(Box::new(settled)) };
        event_loop.run_app(&mut app).map_err(Failed::EventLoop)?;

        match app.stage {
            Stage::Ended(ended) => ended,
            Stage::Settled(_) | Stage::Opening | Stage::Running(_) => Err(Failed::Unended),
        }
    }
}

/// The host, as winit's event loop calls it.
struct App {
    workers: NonZeroUsize,
    stop_within: Duration,
    clock: Monotonic,
    /// What the engine wakes the loop by once the program runs.
    proxy: EventLoopProxy<Woken>,
    stage: Stage,
}

/// Where the program stands in the event loop.
enum Stage {
    /// The loop has not begun: this is the program, its settings read, to bootstrap.
    Settled(Box<Settled<Desktop>>),
    /// The program is being bootstrapped, within the loop's first callback.
    Opening,
    /// The program runs.
    Running(Box<Running>),
    /// The program ended, as this says, and the loop is ending: it ran to its end, or did not.
    Ended(Result<Exit, Failed>),
}

/// A program running in its windows, and what paces its frames: the display or a timer while its clock runs, and the
/// pacer's advances while the pacer holds it (ADR-0045); whether a window's redraw has come since the loop last
/// settled; and the cycles run undrawn while the compositor asks for no window's frame.
struct Running {
    headless: Headless,
    drawing: Box<Drawing>,
    windowing: Hosting,
    /// The cursors made of the images the windows showed, each made once (ADR-0055).
    cursors: MadeCursors<CustomCursor>,
    clockwork: Clockwork,
    clock: Monotonic,
    stopping: Stopping,
    pacing: Pacing,
    redraws: Redraws,
    undrawn: Undrawn,
}

/// Whether a window's redraw has come since the loop last settled: the redraws winit sends together, one for each
/// window the compositor asked a frame of, are drawn as one frame as the loop next settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Redraws {
    /// None has come.
    Unheard,
    /// One has, and a frame is drawn as the loop settles.
    Come,
}

/// Whether the program has been asked to stop, as the host keeps it.
#[derive(Debug, Clone, Copy)]
enum Stopping {
    /// It has not.
    Not,
    /// It has, and `Stop` must finish by this time on the host's clock, in nanoseconds.
    By(u64),
}

/// What the loop does once the engine has stepped.
enum Next {
    /// Waits on the windows' events, and the frame it asked for.
    Wait,
    /// Waits on the windows' events until this time on the host's clock, in nanoseconds, then looks again: when a
    /// capped frame is next due, or when the time a program asked to stop gives `Stop` has passed.
    LookAt(u64),
    /// Ends, the program having ended as this says.
    End(Exit),
}

impl ApplicationHandler<Woken> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let stage = mem::replace(&mut self.stage, Stage::Opening);
        self.stage = match stage {
            Stage::Settled(settled) => match self.open(event_loop, *settled) {
                Ok(running) => Stage::Running(Box::new(running)),
                Err(failed) => Stage::Ended(Err(failed)),
            },
            // The loop resumes again only where a platform suspends it, and the windows stay open through it.
            stage @ (Stage::Opening | Stage::Running(_) | Stage::Ended(_)) => stage,
        };
        if let Stage::Ended(_) = self.stage {
            event_loop.exit();
        }
    }

    /// Routes what winit says of a window to the window capability's window it is, by winit's id for it (ADR-0024).
    fn window_event(&mut self, _: &ActiveEventLoop, native: WindowId, event: WindowEvent) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
        let Some(window) = running.drawing.routes.get(&native).copied() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => running.redrawn(window),
            heard => running.heard(window, heard),
        }
    }

    /// The engine was sent something while the loop waited: the loop steps it as it next settles, which follows.
    fn user_event(&mut self, _: &ActiveEventLoop, _: Woken) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
        running.draw_redrawn(event_loop, self.clock);
        let now = self.clock.nanos();
        match running.settle(event_loop, now, self.stop_within) {
            Next::Wait => event_loop.set_control_flow(ControlFlow::Wait),
            Next::LookAt(at) => {
                let rest = Duration::from_nanos(at.saturating_sub(now));
                #[allow(clippy::disallowed_methods)] // The native host owns the real clock (ADR-0001).
                let wake = Instant::now() + rest;
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
            }
            Next::End(exit) => {
                self.stage = Stage::Ended(Ok(exit));
                event_loop.exit();
            }
        }
    }
}

impl App {
    /// Bootstraps `settled` on the screen of `event_loop`, blocking this thread on each root's bootstrap, which opens
    /// its windows and the GPU on them, makes its runners with what its roots served, on the cores its workers leave
    /// free (ADR-0029), puts its engine on its workers, this thread the first, and starts it, asking for the first
    /// frame. Its frames are paced as the renderer's settings present them, a capped grid starting now.
    fn open(&self, event_loop: &ActiveEventLoop, settled: Settled<Desktop>) -> Result<Running, Failed> {
        let (mut engine, screen) = pollster::block_on(settled.bootstrap(Screen::new(event_loop)))?;
        let (drawing, windowing, runners, clockwork) = screen.into_served().map_err(Failed::Unserved)?;
        engine.set_jobs(Arc::new(runners.runners(self.workers)));
        engine.set_wake(Waker::from(Arc::new(Proxy(Mutex::new(self.proxy.clone())))));
        let mut headless = Headless::new(engine, self.workers);
        headless.engine_mut().start();
        let pacing = Pacing::new(drawing.presenting(), self.clock.nanos());
        drawing.ask_next();

        Ok(Running {
            headless,
            drawing,
            windowing,
            cursors: MadeCursors::default(),
            clockwork,
            clock: self.clock,
            stopping: Stopping::Not,
            pacing,
            redraws: Redraws::Unheard,
            undrawn: Undrawn::Unplanned,
        })
    }
}

impl Running {
    /// Takes in winit's redraw of `window`, as the compositor asked for its next frame: the frame held for it is
    /// presented now, and the next is drawn as the loop next settles, with every other window redrawn by then, in one
    /// frame.
    fn redrawn(&mut self, window: aethereal_window::WindowId) {
        self.drawing.redrawn(window, self.clock.nanos());
        self.redraws = Redraws::Come;
    }

    /// Draws a frame, as the loop settles, when a window's redraw has come since it last did; and, when frames follow
    /// one another and a window presented at once now awaits the compositor with no frame held, one more at once, so
    /// its next frame is drawn ahead and presented as soon as the compositor asks for it.
    fn draw_redrawn(&mut self, event_loop: &ActiveEventLoop, clock: Monotonic) {
        let come = mem::replace(&mut self.redraws, Redraws::Unheard) == Redraws::Come;
        if !come {
            return;
        }
        self.frame(event_loop, clock);
        let undrawn = self.drawing.awaits_undrawn();
        let follows = self.pacing.follows();
        let ahead = undrawn && follows;
        if ahead {
            self.frame(event_loop, clock);
        }
    }

    /// Draws a frame, when the program runs, its clock runs, the pacing draws one now, and a window is drawn: acquires
    /// the frame of each window shown at a size whose redraw is due, in turn, each waiting for the display when
    /// `fifo`, says a cycle is due at the time they were acquired, steps until no actor has anything to run, presents
    /// what the renderer drew in each, and asks for the next frame when frames follow one another. A program asked to
    /// stop draws none, a held one runs its cycles as the pacer owes them, as the loop settles, a capped frame asked
    /// for before its grid's next point waits for it, a window hidden or of no size is handed no frame, and when none
    /// is drawn no cycle runs: the loop then waits on the windows' events, and the frame is asked for again once one
    /// is. The windows the window root asks for are opened and closed between its steps, on `event_loop`.
    fn frame(&mut self, event_loop: &ActiveEventLoop, clock: Monotonic) {
        let running = self.headless.engine().lifecycle() == Lifecycle::Running;
        if !running {
            return;
        }
        // Taken before the clock is read: a program that starts held asked to be as its pacer began, before the first
        // frame, and a cycle run on the chassis's clock first would put a delta no advance chose into its clock.
        let now = Time::from_nanos(clock.nanos());
        let _ = self.clockwork.take(self.headless.engine_mut(), now);
        if !self.clockwork.runs() {
            return;
        }
        let paced = self.pacing.draws(clock.nanos());
        if !paced {
            return;
        }
        let drawn = self.drawing.acquire();
        if !drawn {
            return;
        }
        self.headless.engine_mut().cycle(Time::from_nanos(clock.nanos()));
        self.step_until_idle(event_loop);
        self.drawing.serve(clock.nanos());
        self.undrawn.drawn();
        if self.pacing.follows() {
            self.drawing.ask_next();
        }
    }

    /// Opens and closes what the window root asked for, brings in what was pushed and finished since the last step and
    /// steps, until no actor has anything to run, then parks the workers, as the loop next waits on the display or the
    /// windows' events. Each window asked for is opened or closed between steps, on `event_loop`, before the bring
    /// that brings in what the chassis said of it.
    fn step_until_idle(&mut self, event_loop: &ActiveEventLoop) {
        self.between(event_loop);
        while !self.headless.engine().is_idle() {
            self.headless.step();
            self.between(event_loop);
        }
        self.headless.park();
        // A held cycle that has finished is said to the pacer, whose push wakes the loop to step its turn.
        let _ = self.clockwork.settle(self.headless.engine());
    }

    /// Between steps: opens and closes what the window root asked for, takes what the pacer asked, and brings in what
    /// was pushed and finished since.
    fn between(&mut self, event_loop: &ActiveEventLoop) {
        self.windowing(event_loop);
        let now = Time::from_nanos(self.clock.nanos());
        let resumed = self.clockwork.take(self.headless.engine_mut(), now);
        let follows = self.pacing.follows();
        if resumed && follows {
            self.drawing.ask_next();
        }
        self.headless.engine_mut().bring();
    }

    /// Runs the held cycle the pacer asked for, if any, as a frame: acquires the frame of each window drawn, shown at a
    /// size and its redraw due, says the cycle is due, steps until no actor has anything to run, presents what the
    /// renderer drew, and waits for the GPU to finish it, so what the cycle read back is mapped before the loop next
    /// waits. With no window drawn, as when none is shown or each holds a frame the compositor has not asked for, it
    /// runs undrawn, so an advance never waits on a window coming back (ADR-0045).
    fn held(&mut self, event_loop: &ActiveEventLoop) {
        if !self.clockwork.is_owed() {
            return;
        }
        let drawn = self.drawing.acquire();
        self.clockwork.begin(self.headless.engine_mut());
        self.step_until_idle(event_loop);
        if drawn {
            self.drawing.serve(self.clock.nanos());
            self.drawing.finish();
        }
    }

    /// Between the loop's events, at `now`: polls the device, steps what the windows' events, the sources, and the
    /// jobs brought, and says what the loop does next, as the program's lifecycle stands. While it runs capped, the
    /// loop waits until the next frame is due, and asks for it once it is. Once it is asked to stop, `Stop` has
    /// `stop_within` to finish: the loop waits until that time has passed, woken by what `Stop`'s chain waits on as
    /// it comes in.
    fn settle(&mut self, event_loop: &ActiveEventLoop, now: u64, stop_within: Duration) -> Next {
        self.drawing.poll();
        self.step_until_idle(event_loop);
        let within = u64::try_from(stop_within.as_nanos()).unwrap_or(u64::MAX);
        match (self.headless.engine().lifecycle(), self.stopping) {
            (Lifecycle::Stopped, _) => Next::End(Exit::Stopped),
            (Lifecycle::Stopping, Stopping::Not) => {
                let by = now.saturating_add(within);
                self.stopping = Stopping::By(by);
                Next::LookAt(by)
            }
            (Lifecycle::Stopping, Stopping::By(by)) if now >= by => Next::End(Exit::OutOfTime),
            (Lifecycle::Stopping, Stopping::By(by)) => Next::LookAt(by),
            (Lifecycle::Running, _) if !self.clockwork.runs() => self.owed(event_loop, now),
            (Lifecycle::Running, _) => self.paced(event_loop, now),
            (Lifecycle::Waiting, _) => Next::Wait,
        }
    }

    /// What the loop does next at `now` while the pacer holds the clock: runs a held cycle the pacer asked for, maps
    /// what the renderer read back outside a frame, as a target is read under a held clock (ADR-0048), and waits on
    /// the windows' events and the pacer's next push. A held cycle owed while a window that shows holds a frame drawn
    /// ahead, the one before presented lately, waits for the compositor to ask for it, as a clip recorded at the
    /// display's rate draws each frame; once each window has waited some frames, as a covered window waits on
    /// Wayland, the cycle runs undrawn, and never waits on the window being uncovered. No frame comes to map a read in, so the
    /// loop waits for the GPU to map it, and what became of it wakes the loop.
    fn owed(&mut self, event_loop: &ActiveEventLoop, now: u64) -> Next {
        let owed = self.clockwork.is_owed();
        let next = match self.drawing.hold(now) {
            Hold::WaitUntil { nanos } if owed => Next::LookAt(nanos),
            Hold::WaitUntil { .. } | Hold::RunNow => {
                self.held(event_loop);
                Next::Wait
            }
        };
        let mapping = self.drawing.map();
        if mapping {
            self.drawing.finish();
        }

        next
    }

    /// What the loop does next while the program runs, at `now`, as its frames are paced: waits on the frame asked
    /// for, asks for one due and waits on it, or waits until one is due. A frame due while no window is shown at a
    /// size is asked of none, and the loop waits on the windows' events until one is.
    fn paced(&mut self, event_loop: &ActiveEventLoop, now: u64) -> Next {
        match self.pacing.due(now) {
            Due::Asked => self.unasked(event_loop, now),
            Due::Now => {
                self.drawing.ask_next();
                Next::Wait
            }
            Due::At(at) => Next::LookAt(at),
        }
    }

    /// What the loop does next at `now` while frames follow one another, each asked for as the last is presented:
    /// waits on the frame asked for while a window that shows is due or was presented lately; once every window that
    /// shows has awaited the compositor's ask for some frames, as a covered window does on Wayland, runs the cycles
    /// undrawn on a grid of the budget, so the program's clock runs on and an agent's read of its frame statistics is
    /// answered, and waits on the windows' events until the next is due.
    fn unasked(&mut self, event_loop: &ActiveEventLoop, now: u64) -> Next {
        let silence = self.drawing.silence();
        match self.undrawn.at(silence, self.drawing.budget.nanos(), now) {
            Undrawing::Heard => Next::Wait,
            Undrawing::WaitUntil { nanos } => Next::LookAt(nanos),
            Undrawing::CycleNow { next_nanos } => {
                self.headless.engine_mut().cycle(Time::from_nanos(now));
                self.step_until_idle(event_loop);
                Next::LookAt(next_nanos)
            }
        }
    }
}
