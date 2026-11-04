//! The windows the window root asks for while the program runs, opened, closed, shown in a mode, resized, and moved
//! between steps, inside one of winit's callbacks, where a native window is made (ADR-0027, ADR-0047).

use super::Running;
use crate::{gpu, moding};
use aethereal_window::{Logical, Mode, Place, Surface, Unplaced, Unset, WindowId, Windowing};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;

impl Running {
    /// Takes what the window root asked since the last take, and opens, closes, sets the mode of, resizes, or moves each
    /// window, in the order asked, on `event_loop`.
    pub(super) fn windowing(&mut self, event_loop: &ActiveEventLoop) {
        for request in self.windowing.take() {
            match request {
                Windowing::Open { window, title, size } => {
                    self.drawing.open(event_loop, &self.windowing, window, &title, size);
                }
                Windowing::Close { window } => self.close(window),
                Windowing::SetMode { window, mode } => self.set_mode(window, mode),
                Windowing::Resize { window, size } => self.resize(window, size),
                Windowing::Move { window, at } => self.move_to(window, at),
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

    /// Asks winit to resize `window` to `size` in logical pixels, as the window root asked, then tells the root so, with
    /// its surface at the size winit applied, or as it has it now when the windowing system resizes it later. A window
    /// not open is refused as not open.
    fn resize(&mut self, window: WindowId, size: Logical) {
        let Some(opened) = self.drawing.window_mut(window) else {
            // Refused only once the window root has gone, and no one is then owed an answer.
            let _ = self.windowing.unresized(window, Unplaced::NotOpen);
            return;
        };
        let applied = opened.native.request_inner_size(LogicalSize::new(size.width, size.height));
        let surface = applied.map_or_else(
            || gpu::standing(&opened.native),
            |size| Surface { width: size.width, height: size.height, scale: opened.native.scale_factor() },
        );
        let _ = self.windowing.resized(window, surface);
    }

    /// Moves `window`'s outer frame to `at` in logical pixels, as the window root asked, then tells the root so. Where
    /// winit cannot say where a window is, it cannot place one either, as on Wayland, whose compositor alone places
    /// windows: the move is refused as one the host does not make. A window not open is refused as not open.
    fn move_to(&mut self, window: WindowId, at: Place) {
        let Some(opened) = self.drawing.window_mut(window) else {
            // Refused only once the window root has gone, and no one is then owed an answer.
            let _ = self.windowing.unmoved(window, Unplaced::NotOpen);
            return;
        };
        let placeable = opened.native.outer_position().is_ok();
        if !placeable {
            let _ = self.windowing.unmoved(window, Unplaced::NoHost);
            return;
        }
        opened.native.set_outer_position(LogicalPosition::new(at.x, at.y));
        let _ = self.windowing.moved(window);
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
