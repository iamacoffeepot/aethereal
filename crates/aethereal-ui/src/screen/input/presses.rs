//! Presses in a row (ADR-0056: Widget state is kept by key): a press of the main button soon after and near the last,
//! on the same widget, counts one more, so a text selects a word on a double click and a line on a triple. The window
//! tells a press with no count and no time, so the screen counts them by the time of the turn that took each in.

use super::super::Screen;
use crate::drag::DRAG_THRESHOLD;
use crate::tree::KeyPath;
use aethereal_actor::Time;
use aethereal_data::Plain;
use aethereal_math::Vec2;
use aethereal_window::Button;
use core::time::Duration;

/// How soon after the last a press counts as the next in a row.
const PRESSES_APART: Duration = Duration::from_millis(500);

/// The last press of the main button a screen took in, which the next may follow in a row.
#[derive(Debug, Clone, PartialEq, Plain)]
pub(in crate::screen) enum Presses {
    /// None yet.
    Unpressed,
    /// One on the widget at `path`, at `at` in the screen's pixels, taken in at `time`, the `count`th in a row.
    Pressed { path: KeyPath, at: Vec2, time: Time, count: u32 },
}

impl Screen {
    /// Counts a press of `button` on the widget at `path`, at `at` in the screen's pixels, taken in at `now`: one more
    /// than the last, when it is of the main button on the same widget, within [`PRESSES_APART`] of it and within the
    /// drag threshold of where it was; 1 otherwise.
    pub(super) fn count_press(&mut self, path: &KeyPath, at: Vec2, button: Button, now: Time) -> u32 {
        if button != Button::Main {
            return 1;
        }
        let near = DRAG_THRESHOLD * self.viewport.scale;
        let count = match &self.presses {
            Presses::Pressed { path: last, at: was, time, count } => {
                let same = last == path;
                let soon = now < *time + PRESSES_APART;
                let close = (at - *was).length() <= near;
                let follows = same && soon && close;
                if follows {
                    count + 1
                } else {
                    1
                }
            }
            Presses::Unpressed => 1,
        };
        self.presses = Presses::Pressed { path: path.clone(), at, time: now, count };

        count
    }
}
