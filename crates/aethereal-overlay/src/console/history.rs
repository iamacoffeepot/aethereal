//! The commands entered at the console, stepped through with up and down (ADR-0053: the last 200 commands).

use aethereal_data::Plain;
use core::mem;
use std::collections::VecDeque;

/// The most commands kept: the last 200 (ADR-0053).
pub const MOST_COMMANDS: usize = 200;

/// The commands entered, oldest first, and where up and down have stepped among them.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    commands: VecDeque<String>,
    stepping: Stepping,
}

/// Where up and down have stepped.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
enum Stepping {
    /// At the line being typed: nothing recalled.
    #[default]
    Typing,
    /// At the command numbered `index`, the line that was being typed kept as `draft`, to come back to past the
    /// newest.
    Recalled { index: u32, draft: String },
}

impl History {
    /// Keeps `command` as the newest, unless it is empty or repeats the newest, the oldest leaving past
    /// [`MOST_COMMANDS`]; and steps back to the line being typed.
    pub fn keep(&mut self, command: &str) {
        self.stepping = Stepping::Typing;
        let blank = command.trim().is_empty();
        let repeated = self.commands.back().is_some_and(|newest| newest == command);
        let kept = blank || repeated;
        if kept {
            return;
        }
        self.commands.push_back(String::from(command));
        while self.commands.len() > MOST_COMMANDS {
            self.commands.pop_front();
        }
    }

    /// The command one older than the one recalled, or the newest when none is, keeping `typing`, the line as it
    /// stands, to come back to: none when there is no older.
    pub fn older(&mut self, typing: &str) -> Option<&str> {
        let index = match &self.stepping {
            Stepping::Typing => u32::try_from(self.commands.len()).ok()?.checked_sub(1)?,
            Stepping::Recalled { index, .. } => index.checked_sub(1)?,
        };
        let draft = match mem::take(&mut self.stepping) {
            Stepping::Typing => String::from(typing),
            Stepping::Recalled { draft, .. } => draft,
        };
        self.stepping = Stepping::Recalled { index, draft };

        self.commands.get(index as usize).map(String::as_str)
    }

    /// The command one newer than the one recalled, or the line that was being typed past the newest: none when
    /// nothing is recalled.
    pub fn newer(&mut self) -> Option<String> {
        let Stepping::Recalled { index, draft } = &mut self.stepping else {
            return None;
        };
        let next = *index + 1;
        if let Some(command) = self.commands.get(next as usize) {
            *index = next;
            return Some(command.clone());
        }
        let draft = mem::take(draft);
        self.stepping = Stepping::Typing;

        Some(draft)
    }
}
