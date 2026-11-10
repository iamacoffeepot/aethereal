//! A line entered at the console, run (ADR-0053: Commands are tools): `help` and `clear` are the console's own words,
//! and any other first word names one of the engine's tools, whose sheet reads the rest of the line into its input.

use aethereal_data::{Budget, from_json};
use aethereal_tools::{CommandLine, Output, Sheets, ToolSheet, help_listing, split_words};

/// The console's word that lists every tool, or shows one tool's help.
pub const HELP: &str = "help";

/// The console's word that empties the scrollback.
pub const CLEAR: &str = "clear";

/// The most bytes an input read from a line may take: a line is typed by hand.
const MOST_INPUT_BYTES: usize = 64 * 1024;

/// How many of the nearest names an unknown tool is answered with.
const NEAREST: usize = 3;

/// What a line comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Nothing to do: a blank line.
    Blank,
    /// Text to print: what `help` shows.
    Shown(String),
    /// The scrollback to empty.
    Clear,
    /// A tool to call, with its input's bytes, of the sheets of `revision`, and what it answers with.
    Call { tool: String, revision: u64, input: Vec<u8>, output: Output },
    /// Why the line is not a command, to print as a refusal.
    Refused(String),
}

/// What `line` comes to among the tools of `sheets`, `help` laid out `width` characters wide.
pub fn command(line: &str, sheets: &Sheets, width: usize) -> Command {
    let words = match split_words(line) {
        Ok(words) => words,
        Err(error) => return Command::Refused(error.to_string()),
    };
    let Some((first, rest)) = words.split_first() else {
        return Command::Blank;
    };
    match first.as_str() {
        HELP => help(rest, &sheets.tools, width),
        CLEAR => Command::Clear,
        tool => call(tool, rest, sheets),
    }
}

/// `help`: every tool listed, or the help of the one tool `rest` names.
fn help(rest: &[String], tools: &[ToolSheet], width: usize) -> Command {
    let [name] = rest else {
        return Command::Shown(help_listing(tools, width));
    };
    let Some(sheet) = tools.iter().find(|sheet| sheet.name == *name) else {
        return Command::Refused(unknown(name, tools));
    };
    match CommandLine::of(sheet) {
        Ok(line) => Command::Shown(line.help(width)),
        Err(fault) => Command::Refused(format!("`{name}`'s flags do not fit its input: {fault}")),
    }
}

/// A call of `tool`, its input read from `rest` by its sheet's flags.
fn call(tool: &str, rest: &[String], sheets: &Sheets) -> Command {
    let Some(sheet) = sheets.tools.iter().find(|sheet| sheet.name == tool) else {
        return Command::Refused(unknown(tool, &sheets.tools));
    };
    let line = match CommandLine::of(sheet) {
        Ok(line) => line,
        Err(fault) => return Command::Refused(format!("`{tool}`'s flags do not fit its input: {fault}")),
    };
    let input = line.read(rest).map_err(|error| error.to_string()).and_then(|json| {
        from_json(&sheet.input, json.as_str(), &mut Budget::new(MOST_INPUT_BYTES)).map_err(|error| error.to_string())
    });

    match input {
        Ok(input) => {
            Command::Call { tool: String::from(tool), revision: sheets.revision, input, output: sheet.output.clone() }
        }
        Err(error) => Command::Refused(format!("{tool}: {error}")),
    }
}

/// That no tool is named `name`, with the nearest names the console knows.
fn unknown(name: &str, tools: &[ToolSheet]) -> String {
    let mut known: Vec<(usize, &str)> = tools
        .iter()
        .map(|sheet| sheet.name.as_str())
        .chain([HELP, CLEAR])
        .map(|known| (distance(name, known), known))
        .collect();
    known.sort_unstable();
    let nearest: Vec<String> = known.iter().take(NEAREST).map(|(_, known)| format!("`{known}`")).collect();

    format!("no tool is named `{name}`; the nearest: {}", nearest.join(", "))
}

/// How many characters must be inserted, deleted, or changed to turn `from` into `to`: their edit distance, worked
/// out a row at a time.
fn distance(from: &str, to: &str) -> usize {
    let to: Vec<char> = to.chars().collect();
    let mut above: Vec<usize> = (0..=to.len()).collect();
    for (row, typed) in from.chars().enumerate() {
        let mut current = vec![row + 1; to.len() + 1];
        for (column, known) in to.iter().enumerate() {
            let changed = usize::from(typed != *known);
            current[column + 1] = (above[column] + changed).min(above[column + 1] + 1).min(current[column] + 1);
        }
        above = current;
    }

    above[to.len()]
}
