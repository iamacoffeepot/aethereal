//! The program run on winit's event loop through its lifecycle: started once its windows and GPU are open, cycled each
//! frame the display frees, its windows opened and closed between steps as the window root asks, and stopped when its
//! last window has closed or it quits (ADR-0020, ADR-0024, ADR-0027).

mod window;
mod windowing;

use crate::gpu::{Screen, Unopened};
use crate::serving::Drawing;
use aethereal_actor::Time;
use aethereal_chassis_headless::{Exit, Headless, Monotonic};
use aethereal_substrate::{Clock, Engine, Lifecycle};
use core::error::Error;
use core::fmt;
use core::mem;
use core::num::NonZeroUsize;
use core::time::Duration;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

/// How often the loop looks again at a program asked to stop while `Stop` has not finished: it draws no frame then,
/// so nothing else wakes it, and what `Stop`'s chain waits on, a job or a source, may come in at any time.
const STOPPING_LOOK: Duration = Duration::from_millis(4);

/// The desktop host: the workers it runs the engine on, and the time it gives `Stop`. The windows it opens are the
/// program's to ask for, as it is set up ([`Screen::open_window`]).
#[derive(Debug, Clone)]
pub struct Desktop {
    /// The most workers the engine runs on, the event loop's thread among them.
    pub workers: NonZeroUsize,
    /// How long `Stop` has to finish once the program is asked to stop.
    pub stop_within: Duration,
}

/// What the program makes once its windows and GPU are open: its engine, with the window root and the renderer among
/// its roots, and what the chassis serves the renderer, as [`renderer`](crate::renderer) answers.
pub struct Made {
    pub engine: Engine,
    pub drawing: Drawing,
}

/// Why a program did not run to its end.
#[derive(Debug)]
pub enum Failed<E> {
    /// winit's event loop did not run.
    EventLoop(EventLoopError),
    /// A window, or the GPU on it, did not open.
    Unopened(Unopened),
    /// The program did not make its engine, with this error of its own.
    Program(E),
    /// The event loop ended before the program did: before its windows opened, or with the program still running.
    Unended,
}

impl<E: fmt::Display> fmt::Display for Failed<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(error) => write!(f, "the event loop did not run: {error}"),
            Self::Unopened(unopened) => write!(f, "{unopened}"),
            Self::Program(error) => write!(f, "the program was not made: {error}"),
            Self::Unended => write!(f, "the event loop ended before the program did"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> Error for Failed<E> {}

impl Desktop {
    /// Runs a program on winit's event loop, on this thread, through its lifecycle, then drops it (ADR-0020). Once the
    /// loop begins it hands `program` the [`Screen`], which opens each window the program asks for and the GPU on the
    /// first, and which the program makes the engine with; this thread is then the first of the engine's workers, and
    /// `Start` is sent. Each frame the display frees it says a cycle is due, at the time the frames were acquired, and
    /// steps until no actor has anything to run: one cycle draws every window shown. Once the program is asked to
    /// stop, by its last window closing or an actor's [`quit`](aethereal_actor::Turn::quit), it waits at most
    /// [`stop_within`](Self::stop_within) for `Stop` to finish.
    ///
    /// # Errors
    ///
    /// When the event loop does not run, a window or the GPU does not open, `program` answers an error, or the loop
    /// ends before the program does.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn run<P, E>(self, program: P) -> Result<Exit, Failed<E>>
    where
        P: FnOnce(Screen<'_>) -> Result<Made, E>,
    {
        let event_loop = EventLoop::new().map_err(Failed::EventLoop)?;
        let mut app = App { desktop: self, clock: Monotonic::new(), stage: Stage::Unopened(program) };
        event_loop.run_app(&mut app).map_err(Failed::EventLoop)?;

        match app.stage {
            Stage::Ended(ended) => ended,
            Stage::Unopened(_) | Stage::Opening | Stage::Running(_) => Err(Failed::Unended),
        }
    }
}

/// The host, as winit's event loop calls it.
struct App<P, E> {
    desktop: Desktop,
    clock: Monotonic,
    stage: Stage<P, E>,
}

/// Where the program stands in the event loop.
enum Stage<P, E> {
    /// The loop has not begun: this is what makes the program, opening its windows.
    Unopened(P),
    /// The windows and the program are being opened and made, within the loop's first callback.
    Opening,
    /// The program runs.
    Running(Box<Running>),
    /// The program ended, as this says, and the loop is ending.
    Ended(Result<Exit, Failed<E>>),
}

/// A program running in its windows.
struct Running {
    headless: Headless,
    drawing: Drawing,
    stopping: Stopping,
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
    /// Waits on the windows' events until this time on the host's clock, in nanoseconds, then looks again.
    LookAt(u64),
    /// Ends, the program having ended as this says.
    End(Exit),
}

impl<P, E> ApplicationHandler for App<P, E>
where
    P: FnOnce(Screen<'_>) -> Result<Made, E>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let stage = mem::replace(&mut self.stage, Stage::Opening);
        self.stage = match stage {
            Stage::Unopened(program) => match self.open(event_loop, program) {
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
        match running.settle(event_loop, now, self.desktop.stop_within) {
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

impl<P, E> App<P, E>
where
    P: FnOnce(Screen<'_>) -> Result<Made, E>,
{
    /// Has `program` open its windows and the GPU on them and make the engine with them, puts it on its workers, this
    /// thread the first, and starts it, asking for the first frame.
    fn open(&self, event_loop: &ActiveEventLoop, program: P) -> Result<Running, Failed<E>> {
        let Made { engine, drawing } = program(Screen::new(event_loop)).map_err(Failed::Program)?;
        let mut headless = Headless::new(engine, self.desktop.workers);
        headless.engine_mut().start();
        drawing.ask_next();

        Ok(Running { headless, drawing, stopping: Stopping::Not })
    }
}

impl Running {
    /// Draws a frame, when the program runs and a window is shown at a size: acquires the frame of each window shown
    /// at a size, in turn, each waiting for the display, says a cycle is due at the time they were acquired, steps
    /// until no actor has anything to run, presents what the renderer drew in each, and asks for the next frame. A
    /// program asked to stop draws none, a window hidden or of no size is handed no frame, and when none is shown at a
    /// size none is drawn: the loop then waits on the windows' events, and the frame is asked for again once one is
    /// shown at a size. The windows the window root asks for are opened and closed between its steps, on `event_loop`.
    fn frame(&mut self, event_loop: &ActiveEventLoop, clock: Monotonic) {
        let running = self.headless.engine().lifecycle() == Lifecycle::Running;
        if !running {
            return;
        }
        let drawn = self.drawing.acquire();
        if !drawn {
            return;
        }
        self.headless.engine_mut().cycle(Time::from_nanos(clock.nanos()));
        self.step_until_idle(event_loop);
        self.drawing.serve();
        self.drawing.ask_next();
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
    /// and says what the loop does next, as the program's lifecycle stands. Once it is asked to stop, `Stop` has
    /// `stop_within` to finish, and the loop looks again every [`STOPPING_LOOK`] until it has or the time has passed.
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
            (Lifecycle::Waiting | Lifecycle::Running, _) => Next::Wait,
        }
    }
}
