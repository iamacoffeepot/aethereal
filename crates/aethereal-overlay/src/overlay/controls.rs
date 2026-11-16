//! What the overlay's keys and its line's events do (ADR-0053: The three stops, Commands are tools; ADR-0056: Focus and
//! keys). The backtick, a hotkey of its screen, steps the overlay through its stops; at the console's stop the screen
//! takes every key of the window, whatever has its focus, and the line's text input has the focus, so the backtick,
//! Escape, and Tab come to the overlay before the line hears them, as the console's own: the backtick steps on, Escape
//! closes the console, and Tab completes the word before the caret. Up and Down, which the line passes, step its history, and Page
//! Up and Page Down are handed to the scroll shown. The line's edits keep its text, and Enter runs it. With a session's
//! tab open (ADR-0057), Escape empties the line, or on an empty one stops the session's turn; the tabs, a subagent's
//! fold, and an ask's answers are pressed; and the grip is dragged to size the console. Control and Tab open the next
//! tab, and with Shift the one before, round from the last to the first; Alt and a digit, 1 to 9, the tab at that
//! place, Alt-1 the console's own, while the digit alone types into the line.

use super::Overlay;
use super::chat::Reports;
use super::view::{OverlaySends, prompt_path, scrollback_path, transcript_path};
use crate::chat::Open;
use crate::layout::{LEAST_CONSOLE_SHARE, MOST_CONSOLE_SHARE};
use crate::stop::Stop;
use aethereal_actor::{Fault, Tick, Turn, path};
use aethereal_render::Renderer;
use aethereal_render::stats::{SubscribeFrameStats, SubscribeGpuTimes, UnsubscribeFrameStats, UnsubscribeGpuTimes};
use aethereal_ui::{Controls, First, Held, Key, View, take_in};

/// The key that steps the overlay, by its place on the keyboard: the backtick, left of the 1 (ADR-0053).
const STEP_KEY: &str = "Backquote";

/// The key that closes the console back to the band.
const CLOSE_KEY: &str = "Escape";

/// The key that completes the word before the caret, and with Control opens the next tab.
const COMPLETE_KEY: &str = "Tab";

/// The keys that, with Alt, open the tab at their place: the digits 1 to 9 of the keyboard's top row, Alt-1 the
/// console's own tab.
const TAB_KEYS: [&str; 9] = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9"];

impl Controls for Overlay {
    type Sends = OverlaySends;

    const HOTKEYS: &'static [&'static str] = &[STEP_KEY];

    const FIRST: &'static [First] = &[
        First::Key(STEP_KEY),
        First::Key(CLOSE_KEY),
        First::Key(COMPLETE_KEY),
        First::Alt(TAB_KEYS[0]),
        First::Alt(TAB_KEYS[1]),
        First::Alt(TAB_KEYS[2]),
        First::Alt(TAB_KEYS[3]),
        First::Alt(TAB_KEYS[4]),
        First::Alt(TAB_KEYS[5]),
        First::Alt(TAB_KEYS[6]),
        First::Alt(TAB_KEYS[7]),
        First::Alt(TAB_KEYS[8]),
    ];

    /// The console takes every key of the window while it is open (ADR-0053), though a press on its panel moved focus
    /// off its line until the next `Tick` gives it back.
    fn takes_every_key(&self) -> bool {
        self.stop.console()
    }

    fn key(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, key: Key) -> Result<(), Fault> {
        let console = self.stop.console();
        let switching = console && key.held.control;
        let choosing = console && key.held.alt;
        match key.code.as_str() {
            STEP_KEY => self.step(turn),
            CLOSE_KEY if console => self.escape(turn),
            COMPLETE_KEY if switching => {
                let chat = self.chats.step(key.held.shift);
                self.tell(turn, chat)
            }
            COMPLETE_KEY if console => {
                let caret = self.caret();
                self.console.complete(caret);
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
            digit if choosing => self.choose_by(turn, digit),
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

    /// Runs the line its text input submitted: with the console's own tab open, echoed into the scrollback, and a
    /// command that names a tool asked of the engine's tools root; with a session's open, said to it, or run here and
    /// noted in it (ADR-0057). The scroll shown goes to its newest row.
    #[allow(clippy::needless_pass_by_value, reason = "the text an event hands is taken as a `String`, its type")]
    pub(super) fn enter(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, line: String) -> Result<(), Fault> {
        let (command, reports) = match self.chats.open() {
            Open::Console => (self.console.submitted(&line), Reports::Here),
            Open::Session(session) => self.chatted(turn, session, &line)?,
        };
        let end = Key { code: String::from("End"), key: String::from("End"), repeat: false, held: Held::default() };
        self.scroll(&end, turn)?;

        self.call(command, reports, turn)
    }

    /// Escape: with the console's own tab open, closes the console; with a session's, empties the line typed, or with
    /// none typed stops the session's turn.
    fn escape(&mut self, turn: &mut Turn<'_, Self, OverlaySends>) -> Result<(), Fault> {
        if self.chats.open() == Open::Console {
            self.close();
            return Ok(());
        }
        if !self.console.line().is_empty() {
            self.console.clear_line();
            return Ok(());
        }
        let interrupt = self.chats.interrupt();

        self.tell(turn, interrupt)
    }

    /// Hands `key` to the scroll shown, the scrollback's or the open session's transcript's, which slides by it,
    /// though the line has the screen's focus.
    fn scroll(&mut self, key: &Key, turn: &mut Turn<'_, Self, OverlaySends>) -> Result<(), Fault> {
        let path = match self.chats.open() {
            Open::Console => scrollback_path(),
            Open::Session(_) => transcript_path(),
        };
        let heard = self.ui.press_key(&path, key);
        take_in(self, heard, turn)?;

        Ok(())
    }

    /// Opens the tab at the place of `digit` among the tab keys, the console's own at the first: nothing for any other
    /// key, or a place past the last tab.
    fn choose_by(&mut self, turn: &mut Turn<'_, Self, OverlaySends>, digit: &str) -> Result<(), Fault> {
        let Some(place) = TAB_KEYS.iter().position(|code| *code == digit) else {
            return Ok(());
        };

        self.choose(turn, u32::try_from(place).unwrap_or(u32::MAX))
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

    /// The console's grip dragged `by` logical pixels down: the console taller by as much, within its least and most
    /// share of the window.
    #[allow(clippy::unnecessary_wraps, reason = "an event's method answers a fault, as the library calls it")]
    pub(super) fn resize(&mut self, _: &mut Turn<'_, Self, OverlaySends>, by: f32) -> Result<(), Fault> {
        let viewport = self.ui.viewport();
        let tall = viewport.size.y / viewport.scale;
        if tall > 0.0 {
            self.console_share = (self.console_share + by / tall).clamp(LEAST_CONSOLE_SHARE, MOST_CONSOLE_SHARE);
        }

        Ok(())
    }
}
