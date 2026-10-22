//! The program run on winit's event loop through its lifecycle: started once its window and GPU are open, cycled each
//! frame the display frees, and stopped when its window is closed or it quits (ADR-0020, ADR-0024).

mod window;

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
use winit::dpi::LogicalSize;
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// How often the loop looks again at a program asked to stop while `Stop` has not finished: it draws no frame then,
/// so nothing else wakes it, and what `Stop`'s chain waits on, a job or a source, may come in at any time.
const STOPPING_LOOK: Duration = Duration::from_millis(4);

/// The desktop host: the one window it opens, the workers it runs the engine on, and the time it gives `Stop`.
#[derive(Debug, Clone)]
pub struct Desktop {
    /// The window's title.
    pub title: String,
    /// The window's inner width as it opens, in logical pixels: the screen's scale gives its surface's pixels.
    pub width: u32,
    /// The window's inner height as it opens, in logical pixels.
    pub height: u32,
    /// The most workers the engine runs on, the event loop's thread among them.
    pub workers: NonZeroUsize,
    /// How long `Stop` has to finish once the program is asked to stop.
    pub stop_within: Duration,
}

/// What the program makes once its window and GPU are open: its engine, with the window root and the renderer among
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
    /// The window, or the GPU on it, did not open.
    Unopened(Unopened),
    /// The program did not make its engine, with this error of its own.
    Program(E),
    /// The event loop ended before the program did: before the window opened, or with the program still running.
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
    /// loop begins it opens the window and the GPU on it, and hands them to `program`, which makes the engine with
    /// them; this thread is then the first of the engine's workers, and `Start` is sent. Each frame the display frees
    /// it says a cycle is due, at the time the frame was acquired, and steps until no actor has anything to run; once
    /// the program is asked to stop, by its window's close button or an actor's
    /// [`quit`](aethereal_actor::Turn::quit), it waits at most [`stop_within`](Self::stop_within) for `Stop` to
    /// finish.
    ///
    /// # Errors
    ///
    /// When the event loop does not run, the window or the GPU does not open, `program` answers an error, or the loop
    /// ends before the program does.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn run<P, E>(self, program: P) -> Result<Exit, Failed<E>>
    where
        P: FnOnce(Screen) -> Result<Made, E>,
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
    /// The loop has not begun: this is what makes the program once the window opens.
    Unopened(P),
    /// The window and the program are being opened and made, within the loop's first callback.
    Opening,
    /// The program runs.
    Running(Box<Running>),
    /// The program ended, as this says, and the loop is ending.
    Ended(Result<Exit, Failed<E>>),
}

/// A program running in its window.
struct Running {
    headless: Headless,
    drawing: Drawing,
    /// winit's id for the one window, which every event of the window carries.
    window: WindowId,
    /// Where the pointer was last heard, in the window's surface pixels: its top left until it is heard.
    pointer: (f32, f32),
    visible: Visible,
    stopping: Stopping,
}

/// Whether the window is shown, as winit last said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visible {
    /// It is, at least in part.
    Shown,
    /// It is hidden, minimised, or covered whole: it is handed no frame.
    Occluded,
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
    /// Waits on the window's events, and the frame it asked for.
    Wait,
    /// Waits on the window's events until this time on the host's clock, in nanoseconds, then looks again.
    LookAt(u64),
    /// Ends, the program having ended as this says.
    End(Exit),
}

impl<P, E> ApplicationHandler for App<P, E>
where
    P: FnOnce(Screen) -> Result<Made, E>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let stage = mem::replace(&mut self.stage, Stage::Opening);
        self.stage = match stage {
            Stage::Unopened(program) => match self.open(event_loop, program) {
                Ok(running) => Stage::Running(Box::new(running)),
                Err(failed) => Stage::Ended(Err(failed)),
            },
            // The loop resumes again only where a platform suspends it, and the window stays open through it.
            stage @ (Stage::Opening | Stage::Running(_) | Stage::Ended(_)) => stage,
        };
        if let Stage::Ended(_) = self.stage {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, window: WindowId, event: WindowEvent) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
        let is_ours = running.window == window;
        if !is_ours {
            return;
        }
        match event {
            WindowEvent::RedrawRequested => running.frame(self.clock),
            heard => running.heard(heard),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Stage::Running(running) = &mut self.stage else {
            return;
        };
        let now = self.clock.nanos();
        match running.settle(now, self.desktop.stop_within) {
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
    P: FnOnce(Screen) -> Result<Made, E>,
{
    /// Opens the window and the GPU on it, has `program` make the engine with them, puts it on its workers, this
    /// thread the first, and starts it, asking for the first frame.
    fn open(&self, event_loop: &ActiveEventLoop, program: P) -> Result<Running, Failed<E>> {
        let size = LogicalSize::new(self.desktop.width, self.desktop.height);
        let attributes = Window::default_attributes().with_title(self.desktop.title.clone()).with_inner_size(size);
        let screen = Screen::open(event_loop, attributes).map_err(Failed::Unopened)?;
        let Made { engine, drawing } = program(screen).map_err(Failed::Program)?;
        let mut headless = Headless::new(engine, self.desktop.workers);
        headless.engine_mut().start();
        let window = drawing.window.id();
        drawing.window.request_redraw();

        Ok(Running { headless, drawing, window, pointer: (0.0, 0.0), visible: Visible::Shown, stopping: Stopping::Not })
    }
}

impl Running {
    /// Draws a frame, when the program runs and its window is shown at a size: acquires the surface's frame, which
    /// waits for the display, says a cycle is due at the time it was acquired, steps until no actor has anything to
    /// run, presents what the renderer drew, and asks for the next frame. A program asked to stop draws none, and
    /// neither does a window hidden or of no size: the loop then waits on its events, and the frame is asked for again
    /// once it is shown at a size.
    fn frame(&mut self, clock: Monotonic) {
        let running = self.headless.engine().lifecycle() == Lifecycle::Running;
        let shown = self.visible == Visible::Shown;
        let size = self.drawing.window.inner_size();
        let sized = size.width > 0 && size.height > 0;
        let draws = running && shown && sized;
        if !draws {
            return;
        }
        self.drawing.acquire();
        self.headless.engine_mut().cycle(Time::from_nanos(clock.nanos()));
        self.step_until_idle();
        self.drawing.serve();
        self.drawing.window.request_redraw();
    }

    /// Brings in what was pushed and finished since the last step and steps, until no actor has anything to run, then
    /// parks the workers, as the loop next waits on the display or the window's events.
    fn step_until_idle(&mut self) {
        self.headless.engine_mut().bring();
        while !self.headless.engine().is_idle() {
            self.headless.step();
            self.headless.engine_mut().bring();
        }
        self.headless.park();
    }

    /// Between the loop's events, at `now`: polls the device, steps what the window's events and the jobs brought,
    /// and says what the loop does next, as the program's lifecycle stands. Once it is asked to stop, `Stop` has
    /// `stop_within` to finish, and the loop looks again every [`STOPPING_LOOK`] until it has or the time has passed.
    fn settle(&mut self, now: u64, stop_within: Duration) -> Next {
        self.drawing.poll();
        self.step_until_idle();
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
