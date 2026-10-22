//! The window root bootstrapped on the desktop: the windows its settings list, opened on the screen as the program is
//! set up (ADR-0024, ADR-0029).

use crate::running::Desktop;
use aethereal_data::Plain;
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_window::{InitialWindows, Opening, Windows};

/// The window root's settings on the desktop: its section, `windows` (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.desktop.windows")]
#[settings]
pub struct WindowsSettings {
    /// The windows opened as the program is set up, in order, at least one: the renderer opens the GPU on the first.
    pub windows: Vec<WindowSettings>,
}

/// One window opened as the program is set up: its title, and its inner size in logical pixels.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct WindowSettings {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self { windows: vec![WindowSettings { title: String::from("aethereal"), width: 800, height: 600 }] }
    }
}

/// Opens each window the settings list on the screen, in order, as the window capability's windows `0`, `1`, …,
/// begins the window root with them, its close buttons closing them, and gives the desktop its end, which opens and
/// closes the windows the root asks for while the program runs (ADR-0027). Provides the windows opened.
impl Bootstrap<Desktop> for Windows {
    const SECTION: &'static str = "windows";
    type Settings = WindowsSettings;
    type Needs = ();
    type Provides = (InitialWindows,);

    async fn bootstrap(
        settings: WindowsSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(InitialWindows,), Unbuilt> {
        let mut opening = Opening::new(cx.source(), cx.controller());
        let mut opened = Vec::with_capacity(settings.windows.len());
        for window in &settings.windows {
            opened.push(cx.host().open_window(&mut opening, &window.title, (window.width, window.height))?);
        }
        let mut opened = opened.into_iter();
        let Some(first) = opened.next() else {
            return Err(Unbuilt::Failed("the window root's settings list no window to open".into()));
        };

        let (config, hosting) = opening.config();
        cx.begin(&(), config)?;
        cx.serve(hosting);

        Ok((InitialWindows::new(first, opened.collect()),))
    }
}
