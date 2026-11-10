//! The command line's tests: a line split as a shell splits it, read by a sheet's flags into the input, and an answer
//! printed as it would be typed.

use super::*;
use crate::{ActorLog, LogEntry, LogLevel, LoggedEntry, LogsTool, Output, ReadLog, Tool, ToolSheet};
use aethereal_data::{ActorPath, Budget, Plain, Schema, SchemaNodes, from_bytes, from_json, to_bytes};
use core::error::Error;

type Outcome = Result<(), Box<dyn Error>>;

/// Which tools a describe shows: a variant of no fields, and one of a list.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Shown {
    Names,
    Sheets(Vec<String>),
}

/// A tool's input with one flag of each way it is typed.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.describe")]
struct Describe {
    /// The engine to describe.
    #[flag(position)]
    engine: u32,
    /// Which tools are shown.
    #[flag(short = 's')]
    show: Shown,
    /// Whether each description is shown whole.
    #[flag]
    full: bool,
    /// The most tools shown.
    #[flag(long = "most", short = 'm')]
    most_shown: Option<u32>,
    /// The sizes to show them at.
    #[flag]
    sizes: Vec<[u32; 2]>,
}

/// What a describe answers: a struct holding a list of structs.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.described")]
struct Described {
    engine: u32,
    tools: Vec<Summary>,
    shown: Shown,
}

/// One tool as a describe lists it.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.summary")]
struct Summary {
    name: String,
    size: [u32; 2],
    note: Option<String>,
}

/// The sheet of `describe`, from `Describe` to `Described`.
fn sheet() -> ToolSheet {
    ToolSheet::of::<Describe>(
        "describe",
        "Describes an engine's tools.",
        Output::Value(SchemaNodes::of(&Described::SHAPE)),
    )
}

/// The words of `line`, owned.
fn words(line: &[&str]) -> Vec<String> {
    line.iter().copied().map(String::from).collect()
}

/// The input `line` reads into by `describe`'s flags, decoded as its type.
fn read(line: &[&str]) -> Result<Describe, Box<dyn Error>> {
    let sheet = sheet();
    let json = CommandLine::of(&sheet)?.read(&words(line))?;
    let mut budget = Budget::new(1 << 16);
    let bytes = from_json(&sheet.input, json.as_str(), &mut budget)?;

    Ok(from_bytes(&bytes, &mut budget)?)
}

#[test]
fn a_line_splits_at_whitespace_keeping_quoted_and_escaped_text() -> Outcome {
    // Catches a split that breaks a quoted word at its space, keeps the quotes, drops an empty quoted word, reads an
    // escape inside single quotes, or lets an unclosed quote through as text.
    let split = split_words(r#"  call 'a b' "say \"hi\" \\ \n" c\ d '' x"y"z  "#)?;

    assert_eq!(split, words(&["call", "a b", r#"say "hi" \ \n"#, "c d", "", "xyz"]));
    assert_eq!(split_words("'open"), Err(CommandError::Unclosed { quote: '\'' }));
    assert_eq!(split_words("\"open"), Err(CommandError::Unclosed { quote: '"' }));
    assert_eq!(split_words("end\\"), Err(CommandError::TrailingEscape));
    Ok(())
}

#[test]
fn words_read_into_the_fields_their_flags_name() -> Outcome {
    // Catches a positional word given to the wrong field, a long, `=`, or short flag that sets nothing, a switch that
    // takes the next word, `--no-<name>` left on, a comma list read as one word, an `Option` left out that is not
    // none, and a word after `--` read as a flag.
    let sheets = Shown::Sheets(words(&["capture_frame", "record_clip"]));
    let expected = Describe { engine: 3, show: sheets, full: true, most_shown: Some(5), sizes: vec![[1, 2], [3, 4]] };

    let long = read(&["--full", "3", "-s", "sheets:capture_frame,record_clip", "--most=5", "--sizes", "1x2,3x4"])?;
    let after_dashes =
        read(&["--show", "names", "--no-full", "--sizes", "", "--", "--full"]).err().map(|error| error.to_string());
    let left_out = read(&["0", "--show", "names", "--no-full", "--sizes", ""])?;

    assert_eq!(long, expected);
    assert_eq!(after_dashes, Some(String::from("`engine` takes a number")));
    assert_eq!(left_out, Describe { engine: 0, show: Shown::Names, full: false, most_shown: None, sizes: Vec::new() });
    Ok(())
}

#[test]
fn a_line_missing_a_field_or_misread_is_refused_plainly() -> Outcome {
    // Catches a required field left out that reads as a default, a wrong word read as something, and refusals that
    // do not say which flag or what it takes.
    let sheet = sheet();
    let line = CommandLine::of(&sheet)?;
    let refused = |typed: &[&str]| line.read(&words(typed)).err().map(|error| error.to_string());

    assert_eq!(refused(&["--show", "names", "--full", "--sizes", ""]), Some(String::from("missing `engine`")));
    assert_eq!(refused(&["1", "--full", "--sizes", ""]), Some(String::from("missing `--show`")));
    let show = "`--show` takes `names` or `sheets:<a,b,…>`";
    assert_eq!(refused(&["1", "-s", "every", "--full", "--sizes", ""]), Some(String::from(show)));
    assert_eq!(refused(&["1", "2"]), Some(String::from("nothing takes `2`")));
    assert_eq!(refused(&["1", "--loud"]), Some(String::from("there is no flag `--loud`")));
    Ok(())
}

#[test]
fn a_nested_answer_prints_as_blocks_of_fields() -> Outcome {
    // Catches a struct printed on one line, a list of structs printed as JSON or without the blank line between
    // blocks, blocks not indented beneath their field, a leaf printed other than as its flag would read it, and a none
    // printed rather than left out.
    let tools = vec![
        Summary { name: String::from("capture_frame"), size: [1920, 1080], note: Some(String::from("a frame")) },
        Summary { name: String::from("record_clip"), size: [640, 480], note: None },
    ];
    let described = Described { engine: 2, tools, shown: Shown::Sheets(words(&["capture_frame"])) };
    let schema = SchemaNodes::of(&Described::SHAPE);

    let printed = print_value(&schema, &to_bytes(&described)?, &mut Budget::new(1 << 16))?;

    let expected = "engine: 2\n\
                    tools:\n  \
                      name: capture_frame\n  \
                      size: 1920x1080\n  \
                      note: a frame\n\
                    \n  \
                      name: record_clip\n  \
                      size: 640x480\n\
                    shown: sheets:capture_frame";
    assert_eq!(printed, expected);
    Ok(())
}

#[test]
fn a_line_typed_by_a_real_tool_s_sheet_calls_it_and_its_answer_prints() -> Outcome {
    // Catches a real tool whose flags do not fit its input, a line read into bytes its schema does not decode as typed,
    // and its answer printed other than as the console shows it.
    let sheets = LogsTool::sheets();
    let Some(sheet) = sheets.iter().find(|sheet| sheet.name == "actor_logs") else {
        return Err("the logs tool offers `actor_logs`".into());
    };
    let line = "aether.window:0 --since 4 --level warn --contains 'went wrong' --most 2";
    let mut budget = Budget::new(1 << 16);

    let input = CommandLine::of(sheet)?.input(line, &mut budget)?;

    let expected = ReadLog {
        actor: ActorPath::new("aether.window:0"),
        since: 4,
        level: LogLevel::Warn,
        contains: String::from("went wrong"),
        most: 2,
    };
    assert_eq!(from_bytes::<ReadLog>(&input, &mut budget)?, expected);
    let warned = LoggedEntry::Line { level: LogLevel::Warn, text: String::from("it went wrong") };
    let answer = ActorLog { entries: vec![LogEntry { n: 4, entry: warned }], next: 5, skipped: 0 };
    let Output::Value(output) = &sheet.output else {
        return Err("`actor_logs` answers a value".into());
    };
    let printed = print_value(output, &to_bytes(&answer)?, &mut budget)?;
    let shown =
        "entries:\n  n: 4\n  entry:\n    line:\n      level: warn\n      text: it went wrong\nnext: 5\nskipped: 0";
    assert_eq!(printed, shown);
    Ok(())
}
