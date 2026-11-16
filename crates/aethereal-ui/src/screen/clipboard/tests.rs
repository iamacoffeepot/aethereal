//! Copying, cutting, and pasting through a screen: what a text input hands the clipboard, and what the clipboard's
//! answers, which come in a later turn, do to it then. The clipboard's answers are handed in as the screen's handler
//! hands them; no test reaches the host's clipboard.

use super::super::input::tests::{
    Ev, child, column, hit, key_down, keys, laid, press, raised, screen_with_fonts, text_input, typed, window,
};
use super::super::keys::Role;
use super::super::{Heard, Screen};
use super::{Clipping, Copying, Pasting};
use crate::input::Raised;
use aethereal_actor::Answered;
use aethereal_window::{Copied, KeyEvent, Keyed, Pasted};

/// A screen showing one text input of `text`, at the end of which the main button was pressed, so it has the focus and
/// its caret stands there; and the entry its keys come through.
fn focused(text: &str) -> (Screen, u32) {
    let mut screen = screen_with_fonts();
    laid(&mut screen, column(vec![text_input(text)]), &[]);
    let entry = hit(&screen, &child(0));
    press(&mut screen, entry, 99.0, 5.0);
    laid(&mut screen, column(vec![text_input(text)]), &[]);
    let declared = keys(&screen, Role::Screen);

    (screen, declared)
}

/// The key at `code` pressed with Control held, through the entry `entry`.
fn chord(screen: &mut Screen, entry: u32, code: &str) -> Heard {
    key_down(screen, entry, "ControlLeft", "Control");
    let heard = key_down(screen, entry, code, code);
    let up = KeyEvent::KeyUp { code: String::from("ControlLeft"), key: String::from("Control") };
    screen.keyed(Keyed { window: window(), entry, event: up }, &[]);

    heard
}

/// The key at `code` pressed with Shift held, through the entry `entry`.
fn shifted(screen: &mut Screen, entry: u32, code: &str) {
    key_down(screen, entry, "ShiftLeft", "Shift");
    key_down(screen, entry, code, code);
    let up = KeyEvent::KeyUp { code: String::from("ShiftLeft"), key: String::from("Shift") };
    screen.keyed(Keyed { window: window(), entry, event: up }, &[]);
}

#[test]
fn a_paste_goes_in_at_the_caret_as_it_stands_when_the_clipboard_answers() {
    // Catches a paste typed where the caret stood when it was asked, so an edit made while the clipboard was read is
    // overwritten or split; text pasted at the end whatever the caret; and a pasted text's \r\n line breaks typed as
    // they came, which draw a stray glyph.
    let (mut screen, entry) = focused("gold");
    key_down(&mut screen, entry, "ArrowLeft", "ArrowLeft");
    key_down(&mut screen, entry, "ArrowLeft", "ArrowLeft");
    let asked = chord(&mut screen, entry, "KeyV").clips;
    let edited = raised(typed(&mut screen, entry, "X"));
    laid(&mut screen, column(vec![text_input("goXld")]), &[]);

    let pasted = raised(screen.paste(&child(0), "one\r\ntwo"));

    assert_eq!(asked, vec![Clipping::Paste(child(0))]);
    assert_eq!(edited, vec![Raised::Text(Ev::Changed, String::from("goXld"))]);
    assert_eq!(pasted, vec![Raised::Text(Ev::Changed, String::from("goXone\ntwold"))]);
}

#[test]
fn a_paste_refused_or_come_after_the_focus_left_changes_nothing() {
    // Catches a refusal of the clipboard taken as an empty paste that clears the selection, or as a fault; and a paste
    // that lands in a field the reader has since left.
    let (mut screen, entry) = focused("gold");
    chord(&mut screen, entry, "KeyA");

    let refused = screen.pasted(Answered::Yes(Pasted::Refused), Pasting { path: child(0) }, |_| &[]);
    let refused_lines = refused.lines.len();
    let refused_raised = raised(refused);
    key_down(&mut screen, entry, "Escape", "Escape");
    let after = raised(screen.paste(&child(0), "late"));

    assert_eq!(refused_raised, vec![]);
    assert_eq!(refused_lines, 1, "the refusal is logged");
    assert_eq!(after, vec![]);
}

#[test]
fn copy_and_cut_hand_the_selection_and_a_cut_deletes_it_only_once_written() {
    // Catches a copy or a cut of the whole text rather than the selection; a cut that deletes the selection as it is
    // asked, so a clipboard that refuses the write loses the text; and a cut written that leaves the selection in place.
    let (mut screen, entry) = focused("gold");
    shifted(&mut screen, entry, "ArrowLeft");
    shifted(&mut screen, entry, "ArrowLeft");

    let copied = chord(&mut screen, entry, "KeyC").clips;
    let cut = chord(&mut screen, entry, "KeyX");
    let cut_asked = cut.clips.clone();
    let cut_raised = raised(cut);
    let cut = Copying::Cut { path: child(0) };
    let refused = raised(screen.copied(Answered::Yes(Copied::Refused), cut.clone()));
    let written = raised(screen.copied(Answered::Yes(Copied::Copied), cut));

    assert_eq!(copied, vec![Clipping::Copy(String::from("ld"))]);
    assert_eq!(cut_asked, vec![Clipping::Cut(String::from("ld"), child(0))]);
    assert_eq!(cut_raised, vec![], "nothing is cut before the clipboard holds it");
    assert_eq!(refused, vec![], "a refused write cuts nothing");
    assert_eq!(written, vec![Raised::Text(Ev::Changed, String::from("go"))]);
}
