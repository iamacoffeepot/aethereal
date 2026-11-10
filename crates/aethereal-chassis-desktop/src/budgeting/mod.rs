//! The frame budget on the desktop (ADR-0053: Frame statistics): the frame time the program aims at, from how its
//! frames are presented and the refresh of the display its first window is on, said to the renderer through its front
//! as the program starts, and again whenever that window moves, resizes, or changes its scale, which is when it may be
//! on another display, or as the window that is first closes.

#[cfg(test)]
mod tests;

use crate::Presenting;
use crate::serving::Drawing;
use aethereal_render::stats::{FALLBACK_REFRESH_MILLIHERTZ, FrameBudget, refresh_nanos};
use winit::window::Window;

/// The budget of frames presented as `present`, on a display refreshed `refresh_millihertz` times a thousand seconds:
/// a second over the cap for `capped`; the refresh for `fifo`, which paces the frames to it; and the refresh for
/// `uncapped` too, as a reference, as nothing holds the frames to it. A display that reports no refresh, as winit
/// allows on some platforms, is taken as the fallback, 60 Hz, the most common.
pub fn budget(present: Presenting, refresh_millihertz: Option<u32>) -> FrameBudget {
    let refresh = refresh_nanos(refresh_millihertz.unwrap_or(FALLBACK_REFRESH_MILLIHERTZ));
    match present {
        Presenting::Capped { frames_per_second } => FrameBudget::Limit { nanos: frames_per_second.period_nanos() },
        Presenting::Fifo => FrameBudget::Limit { nanos: refresh },
        Presenting::Uncapped => FrameBudget::Reference { nanos: refresh },
    }
}

/// The refresh of the display `window` is on, as winit reports it: none when it cannot say which display, or that
/// display's refresh.
pub fn refresh_of(window: &Window) -> Option<u32> {
    window.current_monitor()?.refresh_rate_millihertz()
}

impl Drawing {
    /// Says the budget to the renderer again, from the display the first window is on now: as it moves, resizes, or
    /// changes its scale, or as the windows change. With no window open, nothing is said.
    pub(crate) fn rebudget(&self) {
        let Some(first) = self.windows.first() else {
            return;
        };
        self.fronting.budget(budget(self.present, refresh_of(&first.native)));
    }
}
