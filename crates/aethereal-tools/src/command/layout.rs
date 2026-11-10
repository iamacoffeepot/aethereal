//! Text laid out for the console's monospace font, where each character is one column (ADR-0053).

use core::mem;

/// What parts two columns.
pub(super) const GAP: &str = "  ";

/// How many columns `text` takes: one a character.
pub(super) fn width_of(text: &str) -> usize {
    text.chars().count()
}

/// `text` padded with spaces to `width` columns; text as wide or wider is kept as it is.
pub(super) fn padded(text: &str, width: usize) -> String {
    let pad = width.saturating_sub(width_of(text));

    format!("{text}{}", " ".repeat(pad))
}

/// `text` cut to `width` columns, its last kept character `…` when any is cut.
pub(super) fn cut(text: &str, width: usize) -> String {
    let fits = width_of(text) <= width;
    if fits {
        return String::from(text);
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();

    format!("{kept}…")
}

/// The words of `text` filled into lines of at most `width` columns, a word wider than that on a line of its own; no
/// lines for text of no words.
pub(super) fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let started = !line.is_empty();
        let over = width_of(&line) + 1 + width_of(word) > width;
        let breaks = started && over;
        if breaks {
            lines.push(mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    let ended = !line.is_empty();
    if ended {
        lines.push(line);
    }

    lines
}
