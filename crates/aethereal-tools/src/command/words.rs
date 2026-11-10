//! A line split into words as a shell splits it (ADR-0053).

use super::CommandError;
use core::mem;

/// The words of `line`, split as a shell splits them: at runs of whitespace; `'…'` keeps everything between the
/// quotes as it is; `"…"` keeps everything but `\"` and `\\`, which stand for `"` and `\`; and outside quotes `\`
/// keeps the character after it. Quotes join what touches them into one word, and `''` is an empty word.
///
/// # Errors
///
/// [`CommandError::Unclosed`] for a quote not closed, and [`CommandError::TrailingEscape`] for a line that ends in a
/// `\` outside quotes.
pub fn split_words(line: &str) -> Result<Vec<String>, CommandError> {
    let mut words = Vec::new();
    let mut word = String::new();
    // Whether a word has begun: a quote begins one that may stay empty.
    let mut begun = false;
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        match character {
            '\'' => {
                begun = true;
                loop {
                    match characters.next() {
                        Some('\'') => break,
                        Some(quoted) => word.push(quoted),
                        None => return Err(CommandError::Unclosed { quote: '\'' }),
                    }
                }
            }
            '"' => {
                begun = true;
                loop {
                    match characters.next() {
                        Some('"') => break,
                        Some('\\') => match characters.next() {
                            Some(escaped @ ('"' | '\\')) => word.push(escaped),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err(CommandError::Unclosed { quote: '"' }),
                        },
                        Some(quoted) => word.push(quoted),
                        None => return Err(CommandError::Unclosed { quote: '"' }),
                    }
                }
            }
            '\\' => {
                begun = true;
                let escaped = characters.next().ok_or(CommandError::TrailingEscape)?;
                word.push(escaped);
            }
            space if space.is_whitespace() => {
                if begun {
                    words.push(mem::take(&mut word));
                    begun = false;
                }
            }
            other => {
                begun = true;
                word.push(other);
            }
        }
    }
    if begun {
        words.push(word);
    }

    Ok(words)
}
