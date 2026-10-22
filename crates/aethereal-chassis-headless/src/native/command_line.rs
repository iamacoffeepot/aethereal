//! The command line as a layer, read with `clap` from the program's flags, and the file it names (ADR-0028).

use super::{COMMAND_LINE, FILE, NativeError, ProgramName};
use aethereal_data::config::{Flag, FlagName, FlagTable, Form, Overlay, Parts};
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

/// The flag that sets any field by its path.
const SET: &str = "set";

/// The flag that names the file layer.
const CONFIG: &str = "config";

/// The flag that prints the configuration's schema and defaults.
const DESCRIBE: &str = "describe";

/// The long names the command line keeps for itself, `clap`'s `--help` among them.
const RESERVED: [&str; 4] = [SET, CONFIG, DESCRIBE, "help"];

/// The short letter the command line keeps for `-h`.
const HELP_LETTER: char = 'h';

/// What a command line asks of the program.
pub(super) enum Asked {
    /// The help, made from the flags and their fields' doc comments.
    Help(String),
    /// The configuration's schema and defaults.
    Describe,
    /// The program run, with what the command line and the file it names set.
    Run {
        /// What the flags and `--set` set.
        command_line: Overlay,
        /// What the file `--config` names sets: nothing when it names none.
        file: Overlay,
    },
}

/// One thing the command line set, at its place among the arguments.
enum Given<'matches> {
    /// A flag and the text given it.
    Value(&'matches Flag, &'matches str),
    /// A switch, turned on by its flag or off by `--no-<name>`.
    Switch(&'matches Flag, bool),
    /// `--set path=value`.
    Set(&'matches str),
}

/// What `args`, the program's own path first, ask of the program named `name`, whose flags are `table` over the
/// composite `parts`.
pub(super) fn read(
    name: &ProgramName,
    table: &FlagTable,
    parts: &Parts,
    args: &[OsString],
) -> Result<Asked, NativeError> {
    refuse_reserved(table)?;

    let mut command = command(name, table);
    let matches = match command.try_get_matches_from_mut(args) {
        Ok(matches) => matches,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => return Ok(Asked::Help(error.render().to_string())),
        Err(error) => return Err(NativeError::CommandLine(error)),
    };
    let describe = matches.get_flag(DESCRIBE);
    if describe {
        return Ok(Asked::Describe);
    }

    let mut command_line = Overlay::new(COMMAND_LINE);
    for given in given(table, &matches) {
        match given {
            Given::Value(flag, text) => command_line.read(flag, text)?,
            Given::Switch(flag, on) => command_line.switch(flag, on)?,
            Given::Set(text) => command_line.set(parts, text)?,
        }
    }

    let mut file = Overlay::new(FILE);
    if let Some(path) = matches.get_one::<PathBuf>(CONFIG) {
        let text = fs::read_to_string(path).map_err(|error| NativeError::File { path: path.clone(), error })?;
        file.document(&text)?;
    }

    Ok(Asked::Run { command_line, file })
}

/// Refuses a flag that takes a name the command line keeps for itself.
fn refuse_reserved(table: &FlagTable) -> Result<(), NativeError> {
    for flag in table.flags() {
        let long = RESERVED.contains(&flag.long());
        if long {
            return Err(NativeError::Reserved { name: FlagName::Long(String::from(flag.long())), field: flag.path() });
        }
        let short = flag.short() == Some(HELP_LETTER);
        if short {
            return Err(NativeError::Reserved { name: FlagName::Short(HELP_LETTER), field: flag.path() });
        }
    }

    Ok(())
}

/// The command line of the program named `name`: its flags, `--set`, `--config`, `--describe`, and `--help`.
fn command(name: &ProgramName, table: &FlagTable) -> Command {
    let set = Arg::new(SET)
        .long(SET)
        .value_name("PATH=VALUE")
        .action(ArgAction::Append)
        .allow_hyphen_values(true)
        .help("Sets the field at a path, `engine.workers=4`: its value read as its flag would read it, and as JSON for a structure");
    let config = Arg::new(CONFIG)
        .long(CONFIG)
        .value_name("FILE")
        .value_parser(value_parser!(PathBuf))
        .help("Reads settings from a JSON file, beneath the command line and the environment");
    let describe = Arg::new(DESCRIBE)
        .long(DESCRIBE)
        .action(ArgAction::SetTrue)
        .help("Prints the configuration's schema and its defaults, as JSON, and runs nothing");
    let command = Command::new(String::from(name.name()))
        .disable_version_flag(true)
        .args_override_self(true)
        .arg(set)
        .arg(config)
        .arg(describe);

    table.flags().iter().fold(command, |command, flag| {
        let help = format!("{} [env: {}]", flag.doc(), name.variable(flag));
        let named = Arg::new(String::from(flag.long())).long(String::from(flag.long())).short(flag.short()).help(help);
        let switch = flag.form() == &Form::Switch;
        if !switch {
            return command.arg(named.action(ArgAction::Set).allow_negative_numbers(true));
        }

        let negated = format!("no-{}", flag.long());
        let off = Arg::new(negated.clone())
            .long(negated.clone())
            .action(ArgAction::SetTrue)
            .overrides_with(String::from(flag.long()))
            .help(format!("Turns --{} off", flag.long()));
        command.arg(named.action(ArgAction::SetTrue).overrides_with(negated)).arg(off)
    })
}

/// What the command line set, in the order it was given, so a later argument for one place wins.
fn given<'matches>(table: &'matches FlagTable, matches: &'matches ArgMatches) -> Vec<Given<'matches>> {
    let mut given: Vec<(usize, Given<'matches>)> = Vec::new();
    for flag in table.flags() {
        let switch = flag.form() == &Form::Switch;
        if switch {
            let negated = format!("no-{}", flag.long());
            given.extend(typed(matches, flag.long()).map(|index| (index, Given::Switch(flag, true))));
            given.extend(typed(matches, &negated).map(|index| (index, Given::Switch(flag, false))));
            continue;
        }

        let value = matches.get_one::<String>(flag.long());
        let index = matches.index_of(flag.long());
        if let (Some(value), Some(index)) = (value, index) {
            given.push((index, Given::Value(flag, value)));
        }
    }

    let sets = matches.get_many::<String>(SET).into_iter().flatten();
    let indices = matches.indices_of(SET).into_iter().flatten();
    given.extend(indices.zip(sets).map(|(index, text)| (index, Given::Set(text))));

    given.sort_by_key(|(index, _)| *index);
    given.into_iter().map(|(_, given)| given).collect()
}

/// Where the switch `id` was typed, when it was.
fn typed(matches: &ArgMatches, id: &str) -> Option<usize> {
    let typed = matches.value_source(id) == Some(ValueSource::CommandLine);
    if !typed {
        return None;
    }

    matches.index_of(id)
}
