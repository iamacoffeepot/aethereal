//! What the chassis answers the window root's requests with: a page owns its canvases, and the engine in it cannot make
//! one, nor show one fullscreen outside a person's click or key, so every window asked for, every window asked to
//! close, and every mode asked for is refused, `NoHost` (ADR-0027). The cursor asked for over a canvas is shown
//! (ADR-0055).

use crate::canvas::Cursors;
use aethereal_substrate::Engine;
use aethereal_window::{Hosting, Windowing};

/// What the chassis keeps of the engine's window root, if it has one.
pub enum Hosted {
    /// The window root's: its requests are answered between steps, and the cursors it asks for shown over the canvases
    /// by `Cursors`.
    Windows(Hosting, Cursors),
    /// Nothing: the engine has no window root, as a page that hears nothing has.
    Nothing,
}

impl Hosted {
    /// Between steps: shows each cursor the window root asked for since the last, its image's pixels as `engine` holds
    /// them, and answers each window it asked to open, to close, or to show in a mode, `NoHost`, for the next bring.
    pub(crate) fn answer(&mut self, engine: &mut Engine) {
        let Self::Windows(hosting, cursors) = self else {
            return;
        };
        for request in hosting.take() {
            match request {
                Windowing::SetCursor { window, cursor } => cursors.show(engine, window, &cursor),
                // Refused only once the window root has gone, and no one is then owed an answer.
                refused => {
                    let _ = hosting.refuse_one(&refused);
                }
            }
        }
    }
}
