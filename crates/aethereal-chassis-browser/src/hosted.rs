//! What the chassis answers the window root's requests with: a page owns its canvases, and the engine in it cannot make
//! one, so every window asked for and every window asked to close is refused, `NoHost` (ADR-0027).

use aethereal_window::Hosting;

/// What the chassis keeps of the engine's window root, if it has one.
pub enum Hosted {
    /// The window root's: its requests are answered between steps.
    Windows(Hosting),
    /// Nothing: the engine has no window root, as a page that hears nothing has.
    Nothing,
}

impl Hosted {
    /// Between steps: answers each window the window root asked to open or close since the last, `NoHost`, for the
    /// next bring.
    pub(crate) fn refuse(&self) {
        let Self::Windows(hosting) = self else {
            return;
        };
        // Refused only once the window root has gone, and no one is then owed an answer.
        let _ = hosting.refuse();
    }
}
