//! What the overlay's keys and its line's events do (ADR-0053: The three stops, Commands are tools; ADR-0056: Focus and
//! keys). The backtick, a hotkey of its screen, steps the overlay through its stops; at the console's stop the screen
//! takes every key of the window, whatever has its focus, and the line's text input has the focus, so the backtick,
//! Escape, and Tab come to the overlay before the line hears them, as the console's own: the backtick steps on, Escape
//! closes the console, and Tab completes the word before the caret. Up and Down, which the line passes, step its history, and Page
//! Up and Page Down are handed to the scrollback's scroll. The line's edits keep its text, and Enter runs it.

use super::Overlay;
use super::view::{OverlaySends, prompt_path, scrollback_path};
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_actor::{Fault, Tick, Turn, path};
use aethereal_render::Renderer;
use aethereal_render::stats::{SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats, UnsubscribeGpuTimes};
use aethereal_ui::{Controls, Held, Key, View, take_in};

/// The key that steps the overlay, by its place on the keyboard: the backtick, left of the 1 (ADR-0053).
const STEP_KEY: &str = "Backquote";

/// The key that closes the console back to the band.
const CLOSE_KEY: &str = "Escape";

/// The key that completes the word before the caret.
const COMPLETE_KEY: &str = "Tab";

impl Controls for Overlay {
    type Sends = OverlaySends;

    const HOTKEYS: &'static [&'static str] = &[STEP_KEY];

    const FIRST: &'static [&'static str] = &[STEP_KEY, CLOSE_KEY, COMPLETE_KEY];

    /// The console takes every key of the window while it is open (ADR-0053), though a press on its panel moved focus
    /// off its line until the next `Tick` gives it back.
    fn takes_every_key(&self) -> bool {
        self.stop.console()
    }

    fn key(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, key: Key) -> Result<(), Fault> {
        let console = self.stop.console();
        match key.code.as_str() {
            STEP_KEY => self.step(turn),
            CLOSE_KEY if console => {
                self.close();
                Ok(())
            }
            COMPLETE_KEY if console => {
                let (caret, columns) = (self.caret(), self.columns());
                self.console.complete(caret, columns);
                Ok(())
            }
            "ArrowUp" if console => {
                self.console.older();
                Ok(())
            }
            "ArrowDown" if console => {
                self.console.newer();
                Ok(())
            }
            "PageUp" | "PageDown" if console => self.scroll(&key, turn),
            _ => Ok(()),
        }
    }
}

impl Overlay {
    /// Takes in the line as its text input edited it.
    #[allow(clippy::unnecessary_wraps, reason = "an event's method answers a fault, as the library calls it")]
    pub(super) fn edit(&mut self, _: &mut Turn<'_, Self, OverlaySends>, line: String) -> Result<(), Fault> {
        self.console.edited(line);

        Ok(())
    }

    /// Runs the line its text input submitted: echoed into the scrollback, which goes to its newest row, and a command
    /// that names a tool asked of the engine's tools root.
    #[allow(clippy::needless_pass_by_value, reason = "the text an event hands is taken as a `String`, its type")]
    pub(super) fn enter(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, line: String) -> Result<(), Fault> {
        let command = self.console.submitted(&line, self.columns());
        let end = Key { code: String::from("End"), key: String::from("End"), repeat: false, held: Held::default() };
        self.scroll(&end, turn)?;

        self.call(command, turn)
    }

    /// Hands `key` to the scrollback's scroll, which slides by it, though the line has the screen's focus.
    fn scroll(&mut self, key: &Key, turn: &mut Turn<'_, Self, OverlaySends>) -> Result<(), Fault> {
        let heard = self.ui.press_key(&scrollback_path(), key);
        take_in(self, heard, turn)?;

        Ok(())
    }

    /// Steps to the next stop: shown, it hears each `Tick`, each frame's statistics, and the GPU's times; hidden, it
    /// hears neither and draws nothing. Opening the console clears the count of warnings and errors, which the console
    /// shows, and gives its line the screen's focus, so it takes every key; the text the backtick typed is dropped, as
    /// the screen took its key.
    fn step(&mut self, turn: &mut Turn<'_, Self, OverlaySends>) -> Result<(), Fault> {
        let was = self.stop;
        self.stop = was.next();
        match (was.draws(), self.stop.draws()) {
            (false, true) => {
                turn.subscribe::<Tick>();
                turn.to(path::<Renderer>(())).send(&SubscribeFrameStats)?;
                turn.to(path::<Renderer>(())).send(&SubscribeGpuTimes)?;
                self.figures_due_nanos = 0;
                self.logs_due_nanos = 0;
            }
            (true, false) => {
                turn.unsubscribe::<Tick>();
                turn.to(path::<Renderer>(())).send(&UnsubscribeFrameStats)?;
                turn.to(path::<Renderer>(())).send(&UnsubscribeGpuTimes)?;
            }
            (false, false) | (true, true) => {}
        }
        let opened = !was.console() && self.stop.console();
        if opened {
            self.alerts.clear();
            let tree = View::view(self);
            self.ui.rebuilt(tree);
            self.ui.focus(&prompt_path());
        }

        Ok(())
    }

    /// Closes the console back to the band, the line's focus going with it, so the window's keys reach the program
    /// again.
    const fn close(&mut self) {
        self.stop = Stop::Band;
    }

    /// How many characters wide the console's rows are on the window as it is now.
    pub(super) fn columns(&self) -> usize {
        Layout::of(self.ui.viewport()).console_columns()
    }
}
