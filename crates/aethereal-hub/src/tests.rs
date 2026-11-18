use super::{NAME, hub};
use aethereal_chassis_headless::native::{Invocation, Invoked, NativeLayers};

#[test]
fn the_hub_reads_its_sections_and_flags_with_no_two_of_one_name() {
    // Catches a root listed here whose settings suggest a flag another listed root already suggests, left so: the hub
    // would refuse to start, as it did when `Http` and `Docker` each suggested `--window`.
    let invocation = Invocation::new([NAME], Vec::<(String, String)>::new());

    let read = hub().read(NAME, invocation);

    let runs = matches!(read, Ok(Invoked::Run(_)));
    assert!(runs, "the hub reads an invocation of no arguments: {:?}", read.err());
}

/// The hub's unit, as it is deployed.
const UNIT: &str = include_str!("../deploy/aethereal-hub.service");

/// What the deploy fills in the unit's command line, each with a value to test it by: systemd's specifiers and the
/// variables of the host's own file.
const FILLED: [(&str, &str); 5] = [
    ("%h", "/home/hub"),
    ("%U", "1000"),
    ("%G", "1000"),
    ("${SESSION_IMAGE}", "aethereal-session:test"),
    ("${SESSION_CREDENTIAL}", "/srv/hub/claude.env"),
];

/// The arguments of the unit's `ExecStart=`, its lines joined where one ends in `\`, each filled in by [`FILLED`].
fn deployed_args() -> Vec<String> {
    let joined = UNIT.replace("\\\n", " ");
    let exec_start = joined.lines().find_map(|line| line.strip_prefix("ExecStart=")).unwrap_or_default();

    exec_start
        .split_whitespace()
        .map(|word| FILLED.iter().fold(String::from(word), |word, (from, to)| word.replace(from, to)))
        .collect()
}

#[test]
fn the_deployed_command_line_is_one_the_hub_reads() {
    // Catches a flag renamed or removed, or a `--set` key moved, with the deployed unit left naming the old one: the
    // rebuilt hub would refuse its command line and the service crash-loop, as it did when `--mcp-port` became
    // `--set local_stack.port`. The command line is read and configured as running the hub first does, every key and
    // value decoded into its section, which `--describe` does not do: it describes the defaults without reading the
    // `--set` keys, so a key moved would pass it.
    let args = deployed_args();
    let unfilled: Vec<&String> = args.iter().filter(|arg| arg.contains('%') || arg.contains("${")).collect();
    assert!(args.len() > 1, "the unit's ExecStart= names the hub and its arguments: {args:?}");
    assert!(unfilled.is_empty(), "the test fills every specifier and variable the unit uses: {unfilled:?}");

    let invocation = Invocation::new(args, Vec::<(String, String)>::new());
    let read = hub().read(NAME, invocation);

    let Ok(Invoked::Run(program)) = read else {
        panic!("the hub reads the deployed command line: {:?}", read.err());
    };
    let configured = program.configuration();
    assert!(configured.is_ok(), "the hub configures the deployed command line: {:?}", configured.err());
}
