//! The browser as a program's host: its own section, the page its roots bootstrap on, the ends it keeps, and the window
//! root, the renderer, and the controls root bootstrapped on it (ADR-0029).
//!
//! Its bootstrapping awaits: the renderer's opens WebGPU on the canvases the window root opened, which the browser
//! answers with promises. So its [`run`](aethereal_substrate::Program::run) answers a future, [`Starting`], which the
//! page awaits, and which comes to the program [`Started`] on the page.

mod controls;
mod renderer;
mod windows;

pub use windows::WindowsSettings;

use crate::canvas::Shown;
use crate::serving::{Drawing, Serving};
use crate::{Browser, Hosted, Listening, Performance};
use aethereal_data::Plain;
use aethereal_substrate::{Engine, Host, ProgramError, Serves, Settled, Unbuilt};
use aethereal_window::Hosting;
use core::pin::Pin;
use wasm_bindgen::{JsCast, JsError, JsValue};
use web_sys::HtmlCanvasElement;

/// How the browser runs a program: its section, `running`, which holds nothing yet, as the page's animation frames
/// pace it (ADR-0028).
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
#[kind(name = "aether.browser.running")]
#[settings]
pub struct RunningSettings {}

/// The page, as the program is bootstrapped on it: the browser's hosting (ADR-0029). It keeps each canvas the window
/// root's bootstrap opened as a window, which the renderer's reaches through the host to open WebGPU on, and the ends
/// the roots give the browser: the window root's, the renderer's, and the page's listeners.
pub struct Paging {
    canvases: Vec<ShownCanvas>,
    serving: Serving,
    hosted: Hosted,
    listening: Vec<Listening>,
}

/// A canvas opened as a window, as the window root's bootstrap opened it.
pub struct ShownCanvas {
    pub canvas: HtmlCanvasElement,
    pub shown: Shown,
}

impl Paging {
    /// The page with no canvas opened and no end kept.
    const fn new() -> Self {
        Self { canvases: Vec::new(), serving: Serving::Nothing, hosted: Hosted::Nothing, listening: Vec::new() }
    }

    /// Keeps `canvas`, opened as a window, `shown`, for the renderer to draw to.
    fn opened(&mut self, canvas: HtmlCanvasElement, shown: Shown) {
        self.canvases.push(ShownCanvas { canvas, shown });
    }

    /// The canvases opened as windows, in the order opened.
    fn canvases(&self) -> &[ShownCanvas] {
        &self.canvases
    }

    /// The program of `engine`, started, served as the roots asked, and listening as they did: the canvases are the
    /// page's, which its drawing draws to through their surfaces.
    fn start(self, engine: Engine) -> Started {
        let Self { serving, hosted, listening, .. } = self;

        Started { browser: Browser::new(engine, Performance, serving, hosted), _listening: listening }
    }
}

/// The program bootstrapped, as the page awaits it: a future, as the bootstrap of a program on the browser awaits.
pub type Starting = Pin<Box<dyn Future<Output = Result<Started, ProgramError>>>>;

/// A program running on the page: the browser, for the page's `requestAnimationFrame` to call, and the page's
/// listeners the roots gave it, kept for as long as it runs.
pub struct Started {
    browser: Browser<Performance>,
    _listening: Vec<Listening>,
}

impl Started {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds, as
    /// [`Browser::frame`] does.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.browser.frame(timestamp_millis);
    }

    /// The engine, as of the last frame.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        self.browser.engine()
    }
}

impl Host for Browser<Performance> {
    type Running = RunningSettings;
    type Hosting<'host> = Paging;
    type Exit = Starting;
    type Error = ProgramError;

    /// Bootstraps the program on the page, each root's bootstrap awaited before the next begins, then starts it: its
    /// `Start` runs in the first frame the page calls (ADR-0020).
    fn run(settled: Settled<Self>) -> Result<Starting, ProgramError> {
        Ok(Box::pin(async move {
            let (engine, paging) = settled.bootstrap(Paging::new()).await?;

            Ok(paging.start(engine))
        }))
    }
}

impl Serves<Hosting> for Browser<Performance> {
    fn serve(paging: &mut Paging, hosting: Hosting) {
        paging.hosted = Hosted::Windows(hosting);
    }
}

impl Serves<Drawing> for Browser<Performance> {
    fn serve(paging: &mut Paging, drawing: Drawing) {
        paging.serving = Serving::Drawing(Box::new(drawing));
    }
}

impl Serves<Listening> for Browser<Performance> {
    fn serve(paging: &mut Paging, listening: Listening) {
        paging.listening.push(listening);
    }
}

/// What the page did not open, or would not listen to, as `error` says, as a bootstrap's failure.
fn unopened(error: JsError) -> Unbuilt {
    let message = JsValue::from(error).unchecked_into::<js_sys::Error>().message();

    Unbuilt::Unopened(String::from(message).into())
}
