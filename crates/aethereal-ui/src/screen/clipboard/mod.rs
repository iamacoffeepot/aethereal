//! Copying, cutting, and pasting through the host's clipboard (ADR-0056: Copy and paste; ADR-0024: The clipboard). A
//! widget asks it of its [`Input`](crate::Input), and the screen's handler runs a job for each ask, keeping which
//! widget asked:
//!
//! - **a copy** writes the text the widget handed;
//! - **a cut** writes it too, and once the clipboard holds it, deletes the widget's selection by typing nothing over
//!   it, so a write the host refuses loses no text;
//! - **a paste** reads the clipboard, and once it is answered, types what it holds at the widget's caret as it stands
//!   then, so a text edited in between is pasted into as it stands.
//!
//! Each answer reaches its widget only while the widget still has the screen's focus: one that lost it, or left the
//! tree, takes nothing.

#[cfg(test)]
mod tests;

use super::Screen;
use super::input::Heard;
use crate::input::Clip;
use crate::table::Focus;
use crate::tree::KeyPath;
use aethereal_actor::{Answered, Bytes, Level};
use aethereal_data::Plain;
use aethereal_window::{Copied, Pasted};

/// The clipboard was read for the widget at `path`, which pastes what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.pasting")]
pub struct Pasting {
    pub(crate) path: KeyPath,
}

/// The clipboard was written with what a widget copied, or cut.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.copying")]
pub enum Copying {
    /// A copy: nothing follows the write.
    Copy,
    /// A cut from the widget at `path`, whose selection goes once the clipboard holds it.
    Cut { path: KeyPath },
}

/// What a turn of input asks of the clipboard, for the screen's handler to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clipping {
    /// Write this text.
    Copy(String),
    /// Write this text, cut from the widget at this path.
    Cut(String, KeyPath),
    /// Read it, for the widget at this path.
    Paste(KeyPath),
}

/// `text` as a paste types it: each line break, `\r\n` or `\r`, written `\n`, and every other control character but a
/// tab left out, as typed text holds none (ADR-0051).
fn pasteable(text: &str) -> String {
    let broken = text.replace("\r\n", "\n").replace('\r', "\n");

    broken.chars().filter(|character| matches!(character, '\n' | '\t') || !character.is_control()).collect()
}

/// What `clips`, a widget's asks of the clipboard, are as the screen's handler runs them, the widget at `path`.
pub(super) fn clippings(clips: Vec<Clip>, path: &KeyPath) -> impl Iterator<Item = Clipping> + '_ {
    clips.into_iter().map(move |clip| match clip {
        Clip::Copy(text) => Clipping::Copy(text),
        Clip::Cut(text) => Clipping::Cut(text, path.clone()),
        Clip::Paste => Clipping::Paste(path.clone()),
    })
}

impl Screen {
    /// The clipboard was read for the widget `pasting` names, as `answered` says, its text read from its blob by
    /// `read`: what it holds is pasted there ([`Self::paste`]). A clipboard that holds no text pastes nothing; one
    /// refused or unavailable is logged, and nothing changes.
    pub fn pasted<'a>(
        &mut self,
        answered: Answered<Pasted>,
        Pasting { path }: Pasting,
        read: impl Fn(&Bytes) -> &'a [u8],
    ) -> Heard {
        match answered {
            Answered::Yes(Pasted::Text { text }) => self.paste(&path, &String::from_utf8_lossy(read(&text))),
            Answered::Yes(Pasted::Empty) => Heard::default(),
            unread => {
                let mut heard = Heard::default();
                heard.lines.push((Level::Warn, format!("the clipboard was not read for a paste: {unread:?}")));
                heard
            }
        }
    }

    /// Types `text`, the clipboard's, its line breaks and control characters as typed text holds them, into the widget
    /// at `path` at its caret as it stands now, while it has the screen's focus.
    pub(super) fn paste(&mut self, path: &KeyPath, text: &str) -> Heard {
        let mut heard = Heard::default();
        if *self.table.focus() != Focus::On(path.clone()) {
            heard.lines.push((Level::Debug, format!("a paste came after {path:?} lost the focus, and was dropped")));
            return heard;
        }
        let text = pasteable(text);
        self.input(path, &mut heard, |widget, state, input| widget.typed(state, &text, input));

        heard
    }

    /// The clipboard was written with what a widget copied or cut, as `answered` says. Written with a cut, the widget's
    /// selection goes, typed over with nothing, while it still has the screen's focus. One not written is logged, and
    /// a cut's text stays.
    pub fn copied(&mut self, answered: Answered<Copied>, copying: Copying) -> Heard {
        let mut heard = Heard::default();
        match answered {
            Answered::Yes(Copied::Copied) => {}
            unwritten => {
                heard.lines.push((Level::Warn, format!("the clipboard was not written: {unwritten:?}")));
                return heard;
            }
        }
        let Copying::Cut { path } = copying else {
            return heard;
        };
        if *self.table.focus() == Focus::On(path.clone()) {
            self.input(&path, &mut heard, |widget, state, input| widget.typed(state, "", input));
        }

        heard
    }
}
