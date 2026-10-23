//! The windows the window root asks for while the program runs, opened, closed, and shown in a mode between steps,
//! inside one of winit's callbacks, where a native window is made (ADR-0027).

use super::Running;
use crate::{gpu, moding};
use aethereal_window::{Mode, Unset, WindowId, Windowing};
use winit::event_loop::ActiveEventLoop;

impl Running {
    /// Takes what the window root asked since the last take, and opens, closes, or sets the mode of each window, in the
    /// order asked, on `event_loop`.
    pub(super) fn windowing(&mut self, event_loop: &ActiveEventLoop) {
        for request in self.windowing.take() {
            match request {
                Windowing::Open { window, title, size } => {
                    self.drawing.open(event_loop, &self.windowing, window, &title, size);
                }
                Windowing::Close { window } => self.close(window),
                Windowing::SetMode { window, mode } => self.set_mode(window, mode),
            }
        }
    }

    /// Shows `window` in `mode`, as the window root asked, then tells the root so, with its surface as winit has it
    /// now; its `Moded`, when winit already shows it in another mode than last pushed, is pushed first, and otherwise as
    /// it next resizes. A window not open is refused as not open.
    fn set_mode(&mut self, window: WindowId, mode: Mode) {
        let Some(opened) = self.drawing.window_mut(window) else {
            // Refused only once the window root has gone, and no one is then owed an answer.
            let _ = self.windowing.unset(window, Unset::NotOpen);
            return;
        };
        moding::show(&opened.native, mode);
        opened.moded();
        let _ = self.windowing.set(window, gpu::standing(&opened.native));
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
