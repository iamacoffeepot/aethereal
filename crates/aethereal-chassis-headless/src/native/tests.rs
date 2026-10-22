//! The native layers read into a program on a host of the tests' own: their order, the environment's names, and
//! the refusals each names its layer in.

use super::*;
use aethereal_data::Plain;
use aethereal_data::config::{ConfigError, ConfigFault, FlagName};
use aethereal_data::{JsonFault, JsonForm};
use aethereal_substrate::{Builds, EngineSettings, ProgramError, Settled};
use core::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

type Outcome = Result<(), Box<dyn Error>>;

/// A host that keeps the settings it read and runs nothing.
struct Bench {
    engine: EngineSettings,
    running: BenchRunning,
}

/// The bench's own section.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
#[kind(name = "test.native.running")]
#[settings]
struct BenchRunning {
    /// How fast it runs.
    #[flag(long = "pace", short = 'p')]
    pace: u32,
    /// Whether it says what it does.
    #[flag]
    loud: bool,
}

impl Host for Bench {
    type Running = BenchRunning;
    type Hosting = ();
    type Exit = Self;

    fn run(settled: Settled<Self>) -> Result<Self, ProgramError> {
        Self::build(settled)
    }
}

impl Builds for Bench {
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let engine = settled.engine().clone();
        let running = settled.running().clone();
        settled.bootstrap(&mut ())?;

        Ok(Self { engine, running })
    }
}

/// A file of `text`, named for `test`, removed when dropped.
struct File(PathBuf);

impl File {
    fn new(test: &str, text: &str) -> Result<Self, Box<dyn Error>> {
        let path = env::temp_dir().join(format!("aethereal-native-{}-{test}.json", std::process::id()));
        fs::write(&path, text)?;

        Ok(Self(path))
    }

    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The bench program, read from `args` after its name and `variables`.
fn read(
    program: Program<Bench, ()>,
    args: &[&str],
    variables: &[(&str, &str)],
) -> Result<Invoked<Program<Bench, ()>>, NativeError> {
    let args = ["bench"].iter().chain(args).copied();
    program.read("bench", Invocation::new(args, variables.iter().copied()))
}

/// The bench program read from `args` and `variables`, built.
fn built(program: Program<Bench, ()>, args: &[&str], variables: &[(&str, &str)]) -> Result<Bench, Box<dyn Error>> {
    let Invoked::Run(program) = read(program, args, variables)? else {
        return Err("the invocation asks for the program to run".into());
    };

    Ok(program.build()?)
}

/// The layer, the path as text, and the fault of a refusal by a layer.
fn refusal(error: &ConfigError) -> Option<(&str, String, ConfigFault)> {
    let ConfigError::Layer { layer, path, fault } = error else {
        return None;
    };
    Some((layer, path.to_string(), *fault))
}

#[test]
fn the_first_layer_wins_set_in_code_then_the_command_line_the_environment_and_the_file() -> Outcome {
    // Catches the native layers stacked in another order, the code layer put beneath them, and a command line whose
    // later argument for one place does not win over an earlier one.
    let file = File::new("order", r#"{"engine":{"hops":4,"parts":4,"read_limit":4},"running":{"pace":4}}"#)?;
    let program = Bench::program().set_engine(EngineSettings::HOPS.to(1));
    let args = ["--hops", "2", "-p", "9", "--set", "running.pace=2", "--loud", "--no-loud", "--config", &file.path()];
    let variables = [("BENCH_HOPS", "3"), ("BENCH_PACE", "3"), ("BENCH_PARTS", "3"), ("BENCH_LOUD", "true")];

    let bench = built(program, &args, &variables)?;

    let engine = (bench.engine.hops, bench.engine.parts.get(), bench.engine.read_limit, bench.engine.mail_bytes);
    assert_eq!(engine, (1, 3, 4, 1 << 20));
    assert_eq!(bench.running, BenchRunning { pace: 2, loud: false });
    Ok(())
}

#[test]
fn the_environment_reads_only_the_flags_the_program_left() -> Outcome {
    // Catches a variable with the prefix that names no flag read or skipped silently, a hidden field still read
    // from its variable, a renamed flag read under its old name, and a variable that only begins like the prefix
    // taken as the program's.
    let renamed = || Bench::program().flags(|flags| flags.rename(BenchRunning::PACE, "tempo"));
    let others = [("BENCHMARK_PACE", "1"), ("BENCH", "1"), ("PATH", "/")];

    let unknown = read(Bench::program(), &[], &[("BENCH_BOGUS", "1")]).err();
    let hidden = read(Bench::program().flags(|flags| flags.hide(BenchRunning::PACE)), &[], &[("BENCH_PACE", "1")]);
    let old_name = read(renamed(), &[], &[("BENCH_PACE", "1")]).err();
    let bench = built(renamed(), &[], &[others.as_slice(), &[("BENCH_TEMPO", "5")]].concat())?;

    let unknown_variable = |variable: &str| Some(String::from(variable));
    let named = |error: Option<NativeError>| match error {
        Some(NativeError::UnknownVariable { variable }) => Some(variable),
        _ => None,
    };
    assert_eq!(named(unknown), unknown_variable("BENCH_BOGUS"));
    assert_eq!(named(hidden.err()), unknown_variable("BENCH_PACE"));
    assert_eq!(named(old_name), unknown_variable("BENCH_PACE"));
    assert_eq!(bench.running.pace, 5);
    Ok(())
}

#[test]
fn a_set_or_a_file_that_is_refused_names_its_layer_and_path() -> Outcome {
    // Catches a refusal of what `--set` or the file set blamed on another layer, or without the path it set.
    let bad_value = File::new("value", r#"{"running":{"pace":"fast"}}"#)?;
    let not_json = File::new("json", "{")?;
    let missing = env::temp_dir().join(format!("aethereal-native-{}-missing.json", std::process::id()));

    let unknown = built(Bench::program(), &["--set", "running.bogus=1"], &[]).err();
    let no_value = read(Bench::program(), &["--set", "running.pace"], &[]).err();
    let wrong_shape = built(Bench::program(), &["--config", &bad_value.path()], &[]).err();
    let unread = read(Bench::program(), &["--config", &not_json.path()], &[]).err();
    let absent = read(Bench::program(), &["--config", &missing.display().to_string()], &[]).err();

    let program_refusal = |error: Option<Box<dyn Error>>| {
        let error = error?.downcast::<ProgramError>().ok()?;
        let ProgramError::Configuration(error) = *error else {
            return None;
        };
        refusal(&error).map(|(layer, path, fault)| (String::from(layer), path, fault))
    };
    let native_refusal = |error: Option<NativeError>| match error {
        Some(NativeError::Configuration(error)) => {
            refusal(&error).map(|(layer, path, fault)| (String::from(layer), path, fault))
        }
        _ => None,
    };
    let unknown_field = ConfigFault::Json(JsonFault::UnknownField);
    let number = ConfigFault::Json(JsonFault::Expected(JsonForm::Number));
    assert_eq!(
        program_refusal(unknown),
        Some((String::from(COMMAND_LINE), String::from("running.bogus"), unknown_field))
    );
    assert_eq!(native_refusal(no_value), Some((String::from(COMMAND_LINE), String::new(), ConfigFault::NoValue)));
    assert_eq!(program_refusal(wrong_shape), Some((String::from(FILE), String::from("running.pace"), number)));
    let unread = native_refusal(unread).map(|(layer, _, _)| layer);
    assert_eq!(unread.as_deref(), Some(FILE));
    assert!(matches!(absent, Some(NativeError::File { path, .. }) if path == missing));
    Ok(())
}

#[test]
fn help_and_describe_run_nothing() -> Outcome {
    // Catches `--help` or `--describe` taken as a program to run, and help that leaves out a field's doc comment or
    // the variable that reads it.
    let help = read(Bench::program(), &["--help"], &[])?;
    let describe = read(Bench::program(), &["--describe"], &[])?;

    let Invoked::Help(help) = help else {
        return Err("--help asks for the help".into());
    };
    assert!(help.contains("How fast it runs. [env: BENCH_PACE]"), "{help}");
    let Invoked::Describe(describe) = describe else {
        return Err("--describe asks for the description".into());
    };
    assert!(describe.contains(r#""running":{"loud":false,"pace":0}"#), "{describe}");
    Ok(())
}

#[test]
fn a_flag_named_as_the_command_line_keeps_for_itself_is_refused() {
    // Catches a program flag named `config` or `-h` that shadows the command line's own, or panics inside `clap`.
    let long = read(Bench::program().flags(|flags| flags.rename(BenchRunning::PACE, "config")), &[], &[]).err();
    let short = read(
        Bench::program().flags(|flags| flags.hide(BenchRunning::PACE).flag("speed", 'h', BenchRunning::PACE)),
        &[],
        &[],
    )
    .err();

    assert!(matches!(long, Some(NativeError::Reserved { name: FlagName::Long(name), .. }) if name == "config"));
    assert!(matches!(short, Some(NativeError::Reserved { name: FlagName::Short('h'), .. })));
}

#[test]
fn a_started_process_is_not_passed_the_programs_variables() -> Outcome {
    // Catches a child handed the program's configuration through the environment, a variable that only begins like
    // the prefix withheld, and a prefix that keeps the name's `-`.
    let name = ProgramName::new("my-game")?;
    let variables = [("MY_GAME_WORKERS", "4"), ("MY_GAMEPLAY", "1"), ("PATH", "/")];
    let mut command = Command::new("child");

    name.withhold(&mut command, variables.map(|(variable, value)| (OsString::from(variable), OsString::from(value))));

    let withheld: Vec<_> = command.get_envs().collect();
    assert_eq!(withheld, [(OsString::from("MY_GAME_WORKERS").as_os_str(), None)]);
    assert!(ProgramName::new("my game").is_err());
    assert!(ProgramName::new("9lives").is_err());
    Ok(())
}
