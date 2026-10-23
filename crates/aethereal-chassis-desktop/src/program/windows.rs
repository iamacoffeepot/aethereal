//! The window root bootstrapped on the desktop: the windows its settings list, opened on the screen as the program is
//! set up (ADR-0024, ADR-0029).

use crate::running::Desktop;
use aethereal_data::Plain;
use aethereal_data::config::Field;
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_window::{InitialWindows, Mode, Opening, Windows};

/// The window root's settings on the desktop, its section, `windows`: the windows opened as the program is set up, in
/// order, at least one, the renderer opening the GPU on the first (ADR-0028).
///
/// The section is the sequence itself, so the first window's size is `windows[0].size`, and a program offers it as a
/// flag through [`FIRST_SIZE`](Self::FIRST_SIZE), and its mode through [`FIRST_MODE`](Self::FIRST_MODE).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.desktop.windows")]
#[settings]
pub struct WindowsSettings(pub Vec<WindowSettings>);

impl WindowsSettings {
    /// The handle of the first window's size, `windows[0].size`: `.flags(|flags| flags.flag("size", 's',
    /// WindowsSettings::FIRST_SIZE))` offers `--size 1280x720`.
    pub const FIRST_SIZE: Field<Self, [u32; 2]> = WindowSettings::SIZE.in_element(0);

    /// The handle of the first window's mode, `windows[0].mode`: `.flags(|flags| flags.flag("mode", 'm',
    /// WindowsSettings::FIRST_MODE))` offers `--mode fullscreen`.
    pub const FIRST_MODE: Field<Self, Mode> = WindowSettings::MODE.in_element(0);
}

/// One window opened as the program is set up: an element of the window root's settings (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.desktop.window")]
#[settings]
pub struct WindowSettings {
    /// The window's title.
    pub title: String,
    /// The window's inner size in logical pixels, width by height: `1280x720`. A window opened maximized or
    /// fullscreen returns to it once windowed.
    pub size: [u32; 2],
    /// How the window is shown as it opens: `windowed`, at its size; `maximized`, filling the screen's work area; or
    /// `fullscreen`, borderless over the monitor it opens on.
    pub mode: Mode,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self(vec![WindowSettings { title: String::from("aethereal"), size: [800, 600], mode: Mode::Windowed }])
    }
}

/// Opens each window the settings list on the screen, in order, each shown in its mode, as the window capability's
/// windows `0`, `1`, …, begins the window root with them, its close buttons closing them, and gives the desktop its
/// end, which opens and closes the windows the root asks for while the program runs, and sets their modes (ADR-0027).
/// Provides the windows opened.
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
        let mut opened = Vec::with_capacity(settings.0.len());
        for window in &settings.0 {
            opened.push(cx.host().open_window(&mut opening, &window.title, window.size.into(), window.mode)?);
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
