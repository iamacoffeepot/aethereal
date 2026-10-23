//! The program run on winit's event loop through its lifecycle: started once its windows and GPU are open, cycled each
//! frame, as the display frees one, as fast as they are made, or at a rate, its windows opened and closed between
//! steps as the window root asks, and stopped when its last window has closed or it quits (ADR-0020, ADR-0021,
//! ADR-0024, ADR-0027).

mod pacing;
mod window;
mod windowing;

use crate::gpu::Screen;
use crate::serving::Drawing;
use aethereal_actor::Time;
use aethereal_chassis_headless::{Exit, Headless, Monotonic, Runners};
use aethereal_substrate::{Clock, Lifecycle, ProgramError, Settled};
use aethereal_window::Hosting;
use core::error::Error;
use core::fmt;
use core::mem;
use core::num::NonZeroUsize;
use core::time::Duration;
use pacing::{Due, Pacing};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

/// How often the loop looks again at a program asked to stop while `Stop` has not finished: it draws no frame then,
/// so nothing else wakes it, and what `Stop`'s chain waits on, a job or a source, may come in at any time.
const STOPPING_LOOK: Duration = Duration::from_millis(4);

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
        let event_loop = EventLoop::new().map_err(Failed::EventLoop)?;
        let mut app = App { workers, stop_within, clock: Monotonic::new(), stage: Stage::Settled(Box::new(settled)) };
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

/// A program running in its windows, and what paces its frames.
struct Running {
    headless: Headless,
    drawing: Box<Drawing>,
    windowing: Hosting,
    stopping: Stopping,
    pacing: Pacing,
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
    /// capped frame is next due, or when to look again at a program asked to stop.
    LookAt(u64),
    /// Ends, the program having ended as this says.
    End(Exit),
}

impl ApplicationHandler for App {
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
    fn window_event(&mut self, event_loop: &ActiveEventLoop, native: WindowId, event: WindowEvent) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
        let Some(window) = running.drawing.routes.get(&native).copied() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => running.frame(event_loop, self.clock),
            heard => running.heard(window, heard),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
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
    /// its windows and the GPU on them, makes its runners for the job types the program runs, on the cores its
    /// workers leave free (ADR-0029), puts its engine on its workers, this thread the first, and starts it, asking
    /// for the first frame. Its frames are paced as the renderer's settings present them, a capped grid starting now.
    fn open(&self, event_loop: &ActiveEventLoop, settled: Settled<Desktop>) -> Result<Running, Failed> {
        let jobs = settled.jobs().to_vec();
        let (mut engine, screen) = pollster::block_on(settled.bootstrap(Screen::new(event_loop)))?;
        engine.set_jobs(Arc::new(Runners::beside(jobs, self.workers)));
        let (drawing, windowing) = screen.into_served().map_err(Failed::Unserved)?;
        let mut headless = Headless::new(engine, self.workers);
        headless.engine_mut().start();
        let pacing = Pacing::new(drawing.presenting(), self.clock.nanos());
        drawing.ask_next();

        Ok(Running { headless, drawing, windowing, stopping: Stopping::Not, pacing })
    }
}

impl Running {
    /// Draws a frame, when the program runs, the pacing draws one now, and a window is shown at a size: acquires the
    /// frame of each window shown at a size, in turn, each waiting for the display when `fifo`, says a cycle is due at
    /// the time they were acquired, steps until no actor has anything to run, presents what the renderer drew in each,
    /// and asks for the next frame when frames follow one another. A program asked to stop draws none, a capped frame
    /// asked for before its grid's next point waits for it, a window hidden or of no size is handed no frame, and when
    /// none is shown at a size none is drawn: the loop then waits on the windows' events, and the frame is asked for
    /// again once one is shown at a size. The windows the window root asks for are opened and closed between its
    /// steps, on `event_loop`.
    fn frame(&mut self, event_loop: &ActiveEventLoop, clock: Monotonic) {
        let running = self.headless.engine().lifecycle() == Lifecycle::Running;
        if !running {
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
        self.drawing.serve();
        if self.pacing.follows() {
            self.drawing.ask_next();
        }
    }

    /// Opens and closes what the window root asked for, brings in what was pushed and finished since the last step and
    /// steps, until no actor has anything to run, then parks the workers, as the loop next waits on the display or the
    /// windows' events. Each window asked for is opened or closed between steps, on `event_loop`, before the bring
    /// that brings in what the chassis said of it.
    fn step_until_idle(&mut self, event_loop: &ActiveEventLoop) {
        self.windowing(event_loop);
        self.headless.engine_mut().bring();
        while !self.headless.engine().is_idle() {
            self.headless.step();
            self.windowing(event_loop);
            self.headless.engine_mut().bring();
        }
        self.headless.park();
    }

    /// Between the loop's events, at `now`: polls the device, steps what the windows' events and the jobs brought,
    /// and says what the loop does next, as the program's lifecycle stands. While it runs capped, the loop waits until
    /// the next frame is due, and asks for it once it is. Once it is asked to stop, `Stop` has `stop_within` to
    /// finish, and the loop looks again every [`STOPPING_LOOK`] until it has or the time has passed.
    fn settle(&mut self, event_loop: &ActiveEventLoop, now: u64, stop_within: Duration) -> Next {
        self.drawing.poll();
        self.step_until_idle(event_loop);
        let within = u64::try_from(stop_within.as_nanos()).unwrap_or(u64::MAX);
        let look = u64::try_from(STOPPING_LOOK.as_nanos()).unwrap_or(u64::MAX);
        match (self.headless.engine().lifecycle(), self.stopping) {
            (Lifecycle::Stopped, _) => Next::End(Exit::Stopped),
            (Lifecycle::Stopping, Stopping::Not) => {
                let by = now.saturating_add(within);
                self.stopping = Stopping::By(by);
                Next::LookAt(by.min(now.saturating_add(look)))
            }
            (Lifecycle::Stopping, Stopping::By(by)) if now >= by => Next::End(Exit::OutOfTime),
            (Lifecycle::Stopping, Stopping::By(by)) => Next::LookAt(by.min(now.saturating_add(look))),
            (Lifecycle::Running, _) => self.paced(now),
            (Lifecycle::Waiting, _) => Next::Wait,
        }
    }

    /// What the loop does next while the program runs, at `now`, as its frames are paced: waits on the frame asked
    /// for, asks for one due and waits on it, or waits until one is due. A frame due while no window is shown at a
    /// size is asked of none, and the loop waits on the windows' events until one is.
    fn paced(&self, now: u64) -> Next {
        match self.pacing.due(now) {
            Due::Asked => Next::Wait,
            Due::Now => {
                self.drawing.ask_next();
                Next::Wait
            }
            Due::At(at) => Next::LookAt(at),
        }
    }
}
