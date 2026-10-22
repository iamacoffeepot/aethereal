//! What a window hears, as winit tells the loop: each pushed onto that window's feed in the window capability's
//! vocabulary, for the engine's next bring, and what the chassis itself does with a resize, the window shown or hidden,
//! and its close button (ADR-0024).

use super::Running;
use crate::input;
use crate::serving::Visible;
use aethereal_window::{Event, Surface, WindowId};
use winit::event::{ElementState, WindowEvent};

impl Running {
    /// Takes in what the window `window` heard, other than a frame asked for. Positions are winit's physical ones,
    /// which are the surface's pixels, as the surface is the window's inner size. A key winit made up, as some
    /// platforms do for the keys held as the window gains or loses the keyboard, is not pushed, as a page hears none.
    /// What is pushed once the window root has gone reaches nothing, and is refused, which the loop has no use for.
    ///
    /// The close button asks: what it heard is pushed, and the program is asked to stop only when it is the program's
    /// one window, as a window is never closed while the program runs, so the last open is the one asked.
    pub(super) fn heard(&mut self, window: WindowId, event: WindowEvent) {
        let only = self.drawing.windows.len() == 1;
        let Some(opened) = self.drawing.window_mut(window) else {
            return;
        };
        let native = &opened.native;
        let pushed = match event {
            WindowEvent::Resized(size) => {
                opened.surface.resized(size.width, size.height);
                native.request_redraw();
                Event::Resized(Surface { width: size.width, height: size.height, scale: native.scale_factor() })
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = native.inner_size();
                Event::Resized(Surface { width: size.width, height: size.height, scale: scale_factor })
            }
            WindowEvent::CursorMoved { position, .. } => {
                // A window is some thousands of pixels across, which an f32 holds to a small fraction of one.
                #[allow(clippy::cast_possible_truncation)]
                let at = (position.x as f32, position.y as f32);
                opened.pointer = at;
                Event::PointerMoved { x: at.0, y: at.1 }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let ((x, y), button) = (opened.pointer, input::button(button));
                match state {
                    ElementState::Pressed => Event::PointerDown { x, y, button },
                    ElementState::Released => Event::PointerUp { x, y, button },
                }
            }
            WindowEvent::MouseWheel { delta, .. } => input::wheel(opened.pointer, delta, native.scale_factor()),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let (code, key) = (input::code(event.physical_key), input::key(&event.logical_key));
                match event.state {
                    ElementState::Pressed => Event::KeyDown { code, key, repeat: event.repeat },
                    ElementState::Released => Event::KeyUp { code, key },
                }
            }
            WindowEvent::Focused(true) => Event::FocusGained,
            WindowEvent::Focused(false) => Event::FocusLost,
            WindowEvent::Occluded(occluded) => {
                opened.visible = if occluded {
                    Visible::Occluded
                } else {
                    Visible::Shown
                };
                native.request_redraw();
                return;
            }
            WindowEvent::CloseRequested => {
                if only {
                    self.headless.engine_mut().quit();
                }
                Event::CloseRequested
            }
            WindowEvent::Destroyed => {
                self.headless.engine_mut().quit();
                return;
            }
            _ => return,
        };
        opened.feed.push(pushed).ok();
    }
}
