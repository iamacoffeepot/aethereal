//! The windows the window root asks for while the program runs, opened and closed between steps, inside one of winit's
//! callbacks, where a native window is made (ADR-0027).

use super::Running;
use aethereal_window::{WindowId, Windowing};
use winit::event_loop::ActiveEventLoop;

impl Running {
    /// Takes what the window root asked since the last take, and opens or closes each window, in the order asked, on
    /// `event_loop`.
    pub(super) fn windowing(&mut self, event_loop: &ActiveEventLoop) {
        for request in self.windowing.take() {
            match request {
                Windowing::Open { window, title, size } => {
                    self.drawing.open(event_loop, &self.windowing, window, &title, size);
                }
                Windowing::Close { window } => self.close(window),
            }
        }
    }

    /// Closes `window`, and asks the program to stop once its last window has closed, as `quit` does.
    pub(super) fn close(&mut self, window: WindowId) {
        self.drawing.close(&self.windowing, window);
        let last = self.drawing.windows.is_empty();
        if last {
            self.headless.engine_mut().quit();
        }
    }
}
