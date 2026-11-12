//! `TextInput`'s line: where typed text and deletions land, and how the caret moves over characters of more than one
//! byte.

use super::{Line, Selecting, Toward};

#[test]
fn typed_text_goes_in_at_the_caret_and_replaces_the_selection() {
    // Catches text appended at the end whatever the caret, and typing over a selection that inserts beside it rather
    // than in its place, leaving the selected text behind.
    let mut at_caret = Line::new("gold", 2, 2);
    at_caret.typed("XY");

    let mut over_selection = Line::new("gold", 1, 3);
    over_selection.typed("e");

    assert_eq!(at_caret, Line { text: String::from("goXYld"), caret: 4, anchor: 4 });
    assert_eq!(over_selection, Line { text: String::from("ged"), caret: 2, anchor: 2 });
}

#[test]
fn the_caret_steps_over_whole_characters_and_backspace_deletes_one() {
    // Catches a caret moved a byte at a time, which lands inside a character of more than one byte and panics the next
    // edit there; and a Backspace that deletes a byte of one.
    let mut line = Line::new("añb", 4, 4);
    line.moved(Toward::Back, Selecting::Collapse);
    let stepped = line.caret;
    line.delete(Toward::Back);

    assert_eq!(stepped, 3, "back over the one-byte b");
    assert_eq!(line, Line { text: String::from("ab"), caret: 1, anchor: 1 });
}

#[test]
fn a_caret_inside_a_character_is_pulled_back_to_its_start() {
    // Catches a caret kept from a longer text, now inside a character of the text the screen shows, used as it is:
    // slicing there panics.
    let line = Line::new("ñ", 1, 9);

    assert_eq!((line.caret, line.anchor), (0, 2));
}

#[test]
fn shift_extends_the_selection_and_a_plain_move_collapses_it_to_its_edge() {
    // Catches a Shift move that drags the anchor with the caret, so nothing is ever selected; and a plain move from a
    // selection that steps from the caret, rather than collapsing to the selection's edge as text fields do.
    let mut line = Line::new("gold", 1, 1);
    line.moved(Toward::Forward, Selecting::Extend);
    line.moved(Toward::Forward, Selecting::Extend);
    let selected = line.selected();
    line.moved(Toward::Back, Selecting::Collapse);

    assert_eq!(selected, (1, 3));
    assert_eq!((line.caret, line.anchor), (1, 1));
}
