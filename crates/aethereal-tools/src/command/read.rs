//! A line's words read into a tool's input, by its flags (ADR-0053).

use super::flags::Entry;
use super::{CommandError, CommandLine, split_words};
use aethereal_data::{Budget, Json, from_json, write_json_string};
use std::collections::BTreeMap;

/// What a word that starts with `-` names.
enum Named {
    /// The flag of this entry, with the text given after `=` when there is.
    Flag { entry: usize, inline: Inline },
    /// `--no-name`: the switch of this entry, turned off.
    Off { entry: usize },
}

/// The text a long flag is given in its own word, after `=`.
enum Inline {
    /// `--name=value`.
    Given(String),
    /// `--name` alone.
    Absent,
}

impl CommandLine<'_> {
    /// The input `words` state, as JSON of the input's schema.
    ///
    /// A word filled by position fills the next field so marked, in declared order; `--name value`, `--name=value`,
    /// and `-n value` give a field its text; a switch alone turns it on and `--no-name` off; and after `--` every word
    /// is filled by position. Each field's text is read by its form, picked by its schema as a setting's is. A field
    /// that is not an `Option` must be given, and one left out is none. A tool with no flags takes its input as one
    /// JSON word, or none for an input of no fields.
    ///
    /// # Errors
    ///
    /// Why the words are not a call, as [`CommandError`] says for a person to read.
    pub fn read(&self, words: &[String]) -> Result<Json, CommandError> {
        let flagless = self.entries.is_empty();
        if flagless {
            return one_word(words);
        }

        let positions: Vec<usize> =
            self.entries.iter().enumerate().filter(|(_, entry)| entry.positional()).map(|(index, _)| index).collect();
        let mut next_position = 0;
        let mut bare_only = false;
        let mut given: BTreeMap<usize, String> = BTreeMap::new();
        let mut words = words.iter();
        while let Some(word) = words.next() {
            let ends_flags = word == "--" && !bare_only;
            if ends_flags {
                bare_only = true;
                continue;
            }
            let flag = !bare_only && is_flag(word);
            if !flag {
                let Some(&entry) = positions.get(next_position) else {
                    return Err(CommandError::Unwanted { word: word.clone() });
                };
                next_position += 1;
                given.insert(entry, self.value(entry, word)?);
                continue;
            }

            let (entry, text) = match self.named(word)? {
                Named::Off { entry } => (entry, String::from("false")),
                Named::Flag { entry, inline: Inline::Given(text) } => (entry, text),
                Named::Flag { entry, inline: Inline::Absent } => {
                    let switch = self.entry(entry).switch();
                    if switch {
                        (entry, String::from("true"))
                    } else {
                        let Some(text) = words.next() else {
                            return Err(self.unread(entry));
                        };
                        (entry, text.clone())
                    }
                }
            };
            given.insert(entry, self.value(entry, &text)?);
        }

        self.object(&given)
    }

    /// The input the line `line` states, as bytes of the input's schema, ready to call the tool with; what reading
    /// it into bytes holds is charged to `budget`.
    ///
    /// # Errors
    ///
    /// Why the line is not a call, as [`CommandError`] says for a person to read.
    pub fn input(&self, line: &str, budget: &mut Budget) -> Result<Vec<u8>, CommandError> {
        let json = self.read(&split_words(line)?)?;

        from_json(&self.sheet.input, json.as_str(), budget).map_err(|error| CommandError::NotOfSchema { error })
    }

    /// What the word `word`, which starts with `-`, names.
    fn named(&self, word: &str) -> Result<Named, CommandError> {
        let found = word.strip_prefix("--").map_or_else(|| self.short(word), |long| self.long(long));

        found.ok_or_else(|| CommandError::NoFlag { word: String::from(word) })
    }

    /// What the long flag `typed`, without its dashes and with any `=value` after it, names.
    fn long(&self, typed: &str) -> Option<Named> {
        let (name, inline) = typed
            .split_once('=')
            .map_or((typed, Inline::Absent), |(name, text)| (name, Inline::Given(String::from(text))));
        let flag = self.entries.iter().position(|entry| entry.long() == Some(name));
        if let Some(entry) = flag {
            return Some(Named::Flag { entry, inline });
        }

        let switched = name.strip_prefix("no-")?;
        let alone = matches!(inline, Inline::Absent);
        let off = self.entries.iter().position(|entry| entry.switch() && entry.long() == Some(switched));
        off.filter(|_| alone).map(|entry| Named::Off { entry })
    }

    /// What the short flag `word`, `-n`, names.
    fn short(&self, word: &str) -> Option<Named> {
        let mut letters = word.strip_prefix('-')?.chars();
        let letter = letters.next()?;
        let alone = letters.next().is_none();
        let entry = self.entries.iter().position(|entry| entry.short() == Some(letter));

        entry.filter(|_| alone).map(|entry| Named::Flag { entry, inline: Inline::Absent })
    }

    /// The entry at `index`, which the line's own lookups found.
    fn entry(&self, index: usize) -> &Entry<'_> {
        &self.entries[index]
    }

    /// The JSON of `text` given the field of the entry at `index`, read by its form.
    fn value(&self, index: usize, text: &str) -> Result<String, CommandError> {
        self.entry(index).form.json_of(text).map_err(|_| self.unread(index))
    }

    /// The refusal of text the entry at `index` does not read, saying what it does.
    fn unread(&self, index: usize) -> CommandError {
        let entry = self.entry(index);

        CommandError::Unread { flag: entry.named(), expects: entry.form.expects() }
    }

    /// The input object of the fields `given`, each by its entry, refusing a field that must be given and is not.
    fn object(&self, given: &BTreeMap<usize, String>) -> Result<Json, CommandError> {
        let mut text = String::from("{");
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(value) = given.get(&index) else {
                if entry.required {
                    return Err(CommandError::Missing { flag: entry.named() });
                }
                continue;
            };
            let later = text.len() > 1;
            if later {
                text.push(',');
            }
            write_json_string(&entry.flag.field, &mut text);
            text.push(':');
            text.push_str(value);
        }
        text.push('}');

        Json::parse(text, &mut Budget::new(usize::MAX)).map_err(|error| CommandError::NotJson { error })
    }
}

/// The input of a tool with no flags: its one JSON word, or an object of no fields for none.
fn one_word(words: &[String]) -> Result<Json, CommandError> {
    let text = match words {
        [] => "{}",
        [word] => word.as_str(),
        _ => return Err(CommandError::NotOneWord),
    };

    // The text is in hand, so what reading it holds grows with its length and needs no budget of its own.
    Json::parse(text, &mut Budget::new(usize::MAX)).map_err(|error| CommandError::NotJson { error })
}

/// Whether `word` names a flag: it starts with `-` and is not `-` alone or a negative number, `-5` or `-.5`.
fn is_flag(word: &str) -> bool {
    let mut characters = word.chars();
    let dashed = characters.next() == Some('-');
    let next = characters.next();
    let named = next.is_some_and(|character| !character.is_ascii_digit() && character != '.');

    dashed && named
}
