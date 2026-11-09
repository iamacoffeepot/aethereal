//! The windows the window root asks for while the program runs, opened, closed, shown in a mode, resized, moved, and
//! their input methods allowed or stopped, between steps, inside one of winit's callbacks, where a native window is
//! made (ADR-0027, ADR-0047, ADR-0051).

use super::Running;
use crate::input::Composition;
use crate::{gpu, moding, sizing};
use aethereal_window::{Logical, Mode, Place, Unplaced, Unset, WindowId, Windowing};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;

impl Running {
    /// Takes what the window root asked since the last take, and opens, closes, sets the mode of, resizes, moves, or
    /// allows or stops the input method of each window, in the order asked, on `event_loop`.
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
                Windowing::AllowIme { window, allowed } => self.allow_ime(window, allowed),
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
    /// its surface at the size winit applied, or as it has it now when the windowing system resizes it later. A size
    /// winit applies at once, as Wayland's is, comes with no `Resized`, so the chassis takes it in as one: the surface
    /// is configured to it and the window's subscribers hear it. A size winit answers unchanged and not as asked, as
    /// Wayland's winit answers for a window the compositor holds tiled, was not applied, and is refused as tiled. A window
    /// not open is refused as not open.
    fn resize(&mut self, window: WindowId, size: Logical) {
        let Some(opened) = self.drawing.window_mut(window) else {
            // Refused only once the window root has gone, and no one is then owed an answer.
            let _ = self.windowing.unresized(window, Unplaced::NotOpen);
            return;
        };

        let before = opened.native.inner_size();
        let asked = LogicalSize::new(size.width, size.height);
        let applied = opened.native.request_inner_size(asked);
        if let Some(applied) = applied {
            let wanted = sizing::physical(asked, opened.native.scale_factor());
            if sizing::held(before, applied, wanted) {
                let _ = self.windowing.unresized(window, Unplaced::Tiled);
                return;
            }
            opened.resized(applied.width, applied.height);
        }
        let _ = self.windowing.resized(window, gpu::standing(&opened.native));
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

    /// Lets `window`'s input method compose, or stops it, as the window root asked, then tells the root so. A stop
    /// drops a composition open, uncommitted, as winit's `Disabled` says, so the window takes its composition as closed
    /// then, whether or not the platform says `Disabled`, and a key's text is not held back for good. A window not open
    /// is refused as not open.
    fn allow_ime(&mut self, window: WindowId, allowed: bool) {
        let Some(opened) = self.drawing.window_mut(window) else {
            // Refused only once the window root has gone, and no one is then owed an answer.
            let _ = self.windowing.ime_unallowed(window, Unset::NotOpen);
            return;
        };
        opened.native.set_ime_allowed(allowed);
        if !allowed {
            opened.composition = Composition::Closed;
        }
        let _ = self.windowing.ime_allowed(window);
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
