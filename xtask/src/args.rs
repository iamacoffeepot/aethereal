//! The command line, read into the command it asks for.

use crate::affected;
use crate::check::{Job, Tests};
use crate::failure::Failure;
use crate::session_image::{self, Base, Claude, Request, Tag};
use std::collections::BTreeSet;

/// The port `--serve` listens on when no `--port` names one.
const DEFAULT_PORT: u16 = 8000;

const USAGE: &str = "usage: cargo xtask example <example> [--serve [--port <port>]]
       cargo xtask check [fmt | clippy | browser | test]... [-p <package>]...
       cargo xtask affected [--base <ref>] [--github-output] [--run]
       cargo xtask create-session-image --claude <version | latest | stable> [--uid <uid>] [--gid <gid>]
           [--rust <version>] [--tag <tag>]";

/// What the command line asks for.
pub enum Command {
    /// One browser example's page built, and served when asked.
    Example { name: String, serving: Serving },
    /// The checks named, in CI's order, every check when none is named; and whose tests the test check runs.
    Check { jobs: Vec<Job>, tests: Tests },
    /// The packages a change selects the tests of, printed, and written or run when asked.
    Affected(affected::Request),
    /// The image Claude sessions run in, made as asked.
    CreateSessionImage(Request),
}

/// Whether a built example's page is served, and on which port of localhost.
#[derive(Clone, Copy)]
pub enum Serving {
    Not,
    At(u16),
}

/// Reads the command line, the program's own name left off.
pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, Failure> {
    let command = args.next();
    match command.as_deref() {
        Some("example") => example(args),
        Some("check") => check(args),
        Some("affected") => affected(args),
        Some("create-session-image") => create_session_image(args),
        _ => Err(Failure::usage(USAGE)),
    }
}

fn example(mut args: impl Iterator<Item = String>) -> Result<Command, Failure> {
    let mut names = Vec::new();
    let mut serve = false;
    // The port `--port` named, where it named one.
    let mut port = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--serve" => serve = true,
            "--port" => {
                port = Some(args.next().and_then(|port| port.parse().ok()).ok_or_else(|| Failure::usage(USAGE))?);
            }
            flag if flag.starts_with('-') => return Err(Failure::usage(USAGE)),
            _ => names.push(arg),
        }
    }

    let [name] = <[String; 1]>::try_from(names).map_err(|_| Failure::usage(USAGE))?;
    let serving = match (serve, port) {
        (true, port) => Serving::At(port.unwrap_or(DEFAULT_PORT)),
        (false, None) => Serving::Not,
        (false, Some(_)) => return Err(Failure::usage(USAGE)),
    };
    Ok(Command::Example { name, serving })
}

fn check(mut args: impl Iterator<Item = String>) -> Result<Command, Failure> {
    let mut named = Vec::new();
    let mut packages = BTreeSet::new();
    while let Some(arg) = args.next() {
        if arg == "-p" {
            packages.insert(args.next().ok_or_else(|| Failure::usage(USAGE))?);
        } else {
            named.push(Job::named(&arg).ok_or_else(|| Failure::usage(USAGE))?);
        }
    }

    let jobs: Vec<Job> = if named.is_empty() {
        Job::ALL.to_vec()
    } else {
        Job::ALL.into_iter().filter(|job| named.contains(job)).collect()
    };
    let tests = if packages.is_empty() {
        Tests::Workspace
    } else {
        Tests::Packages(packages)
    };
    // `-p` narrows the test check alone: with no test check to narrow, it is a mistake, not a no-op.
    let narrows_nothing = matches!(tests, Tests::Packages(_)) && !jobs.contains(&Job::Test);
    if narrows_nothing {
        return Err(Failure::usage(format_args!("refused -p: it names whose tests the test check runs\n{USAGE}")));
    }
    Ok(Command::Check { jobs, tests })
}

fn affected(mut args: impl Iterator<Item = String>) -> Result<Command, Failure> {
    let mut base = affected::Base::MergeBase;
    let mut github_output = false;
    let mut run = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base" => base = affected::Base::Named(args.next().ok_or_else(|| Failure::usage(USAGE))?),
            "--github-output" => github_output = true,
            "--run" => run = true,
            _ => return Err(Failure::usage(USAGE)),
        }
    }
    Ok(Command::Affected(affected::Request { base, github_output, run }))
}

fn create_session_image(mut args: impl Iterator<Item = String>) -> Result<Command, Failure> {
    // What `--claude` and `--rust` named, where they named it: `--claude` must, and `--rust` defaults to the
    // repository's toolchain.
    let mut claude = None;
    let mut rust = None;
    let mut uid = session_image::DEFAULT_ID;
    let mut gid = session_image::DEFAULT_ID;
    let mut tag = Tag::Versioned;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| Failure::usage(USAGE))?;
        match flag.as_str() {
            "--claude" => claude = Some(Claude::read(&value)?),
            "--rust" => rust = Some(Base::pinned(&value)?),
            "--uid" => uid = id(&value)?,
            "--gid" => gid = id(&value)?,
            "--tag" => tag = Tag::Named(value),
            _ => return Err(Failure::usage(USAGE)),
        }
    }

    let claude = claude.ok_or_else(|| {
        Failure::usage(format_args!("refused: --claude names the Claude Code the image holds\n{USAGE}"))
    })?;
    let base = match rust {
        Some(base) => base,
        None => Base::toolchain()?,
    };
    Ok(Command::CreateSessionImage(Request { claude, uid, gid, base, tag }))
}

/// The uid or gid `value` writes.
fn id(value: &str) -> Result<u32, Failure> {
    value.parse().map_err(|_| Failure::usage(format_args!("refused id={value}: a uid or gid is a number\n{USAGE}")))
}
