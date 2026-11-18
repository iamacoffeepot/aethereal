//! Files read by path: a change to a file a source reads selects at least what a change to that source selects.
//!
//! A source can read another file with no dependency edge saying so: at compile time (`include_bytes!`,
//! `include_str!`, `include!`, `#[path]`), or at run time, opening a path its tests or its build script name. The file
//! read is then part of the reader's content: editing it changes what the reader's build or tests do. So a change to
//! it must select every test a change to the reader would, and this check asks the selection itself whether it does,
//! for every path-shaped string literal in every member's sources and doc comments that names a file or directory in
//! the workspace.
//!
//! Asking the selection, rather than restating why it should hold, covers every way it can fail at once:
//!
//! - a read across packages the reader does not depend on, which no edge leads back from;
//! - a read from outside a `tests/` tree into one, which [`crate::affected::test_targets`] narrows to its package alone,
//!   with no dependent, and a `#[path]` reaching across a package's root;
//! - a read of a file the path rules ignore (`docs/**`, `**/*.md`, a `README`), which selects nothing at all.
//!
//! A reader whose own change selects every package is skipped: it is the selection's own machinery, whose literals
//! name paths as data, and any change to it runs everything. A directory read that holds a member's root scans the
//! workspace, and only a [`WORKSPACE_SCANNERS`] package may do that, as only those run with every selection.
//!
//! A compile-time read must name its file in a form this check can follow; [`include_arguments`] fails on the rest. A
//! run-time read built from pieces (`format!("{root}/x")`) is beyond it: a path is named whole, as one literal.

use crate::affected::invariants::source::{self, RustSource, string_literals};
use crate::affected::invariants::workspace::{Members, Selections, package_root};
use crate::affected::rules;
use crate::affected::select::{Selection, WORKSPACE_SCANNERS};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::slice;

/// The macros whose argument is a path read at compile time.
const INCLUDE_MACROS: &[&str] = &["include!", "include_str!", "include_bytes!"];

/// A file or directory a source names.
struct Reference {
    /// Where the literal naming it starts in the source.
    offset: usize,
    /// The file or directory, absolute and normalized.
    target: PathBuf,
}

#[test]
fn a_change_to_a_file_read_by_path_selects_what_a_change_to_its_reader_does() {
    let members = Members::load();
    let mut selections = Selections::new(&members);
    let roots: Vec<PathBuf> = members.iter().map(|package| package_root(package).to_path_buf()).collect();
    let mut violations = Vec::new();
    for package in members.iter() {
        let root = package_root(package);
        let scans = WORKSPACE_SCANNERS.contains(&package.name());
        for path in source::walk(root).rust_files {
            let reader = members.relative(&path);
            let machinery = rules::screen(slice::from_ref(&reader)).is_some();
            if machinery {
                continue;
            }
            let Some(file) = RustSource::read(&path) else {
                continue;
            };

            let needed = selections.of(&members, &path).clone();
            for reference in references(&file, root, members.root()) {
                let target = members.relative(&reference.target);
                let line = file.line_of(reference.offset);
                if !reference.target.is_dir() {
                    let got = selections.of(&members, &reference.target);
                    if !got.covers(&needed) {
                        violations.push(missed(&reader, line, &target, &target, got, &needed));
                    }
                    continue;
                }

                let holds_a_member = roots.iter().any(|member| member.starts_with(&reference.target));
                if holds_a_member {
                    if !scans {
                        violations.push(format!(
                            "  {reader}:{line} reads the directory {target}, which holds a member's root: it scans \
                             the workspace.\n    A change to any package can then fail {name}'s tests, so {name} \
                             must run with every selection: add it to WORKSPACE_SCANNERS.",
                            name = package.name(),
                        ));
                    }
                    continue;
                }
                // A directory's `.gitignore` says what git leaves out of it, not what its reader reads.
                let contents =
                    source::walk(&reference.target).files.into_iter().filter(|file| !file.ends_with(".gitignore"));
                for inside in contents {
                    let got = selections.of(&members, &inside);
                    if !got.covers(&needed) {
                        let file = members.relative(&inside);
                        violations.push(missed(&reader, line, &target, &file, got, &needed));
                        break;
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a file is read by path where its change selects less:\n\n{}\n",
        violations.join("\n\n")
    );
}

#[test]
fn every_compile_time_read_names_its_file_in_a_form_the_check_follows() {
    // Catches an `include_bytes!` or `#[path]` whose file the check above cannot work out: it would go unchecked, and a
    // change to the file it reads could select less than its reader without the check failing.
    let members = Members::load();
    let mut unfollowed = Vec::new();
    for package in members.iter() {
        let root = package_root(package);
        let built = build_environment(root);
        for path in source::walk(root).rust_files {
            let Some(file) = RustSource::read(&path) else {
                continue;
            };
            for (offset, form) in include_arguments(&file.code, &built) {
                unfollowed.push(format!("  {}:{}\n    {form}", members.relative(&path), file.line_of(offset)));
            }
        }
    }

    assert!(
        unfollowed.is_empty(),
        "a compile-time read names its file in a form the references check cannot follow:\n\n{}\n\n  Write the path as \
         one string literal, relative to the source or, in `concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/…\")`, to the \
         package's root; or as the package's own build script's output, `concat!(env!(\"OUT_DIR\"), \"/…\")` or an \
         `env!` its build script sets.\n",
        unfollowed.join("\n\n")
    );
}

/// The violation for a reader at `reader:line` naming `target`, of which a change to `file` selects `got` where a
/// change to the reader selects `needed`.
fn missed(reader: &str, line: usize, target: &str, file: &str, got: &Selection, needed: &Selection) -> String {
    format!(
        "  {reader}:{line} reads {target}.\n    A change to {file} selects {got:?}; a change to {reader} selects \
         {needed:?}.\n    Fix: make the reader's package depend on the package the file sits in, or move the file \
         into the reader's package, out of any `tests/` directory and any path the rules ignore."
    )
}

/// Every file or directory in the workspace that a string literal in `file`'s code or doc comments names, relative to
/// the file's directory (an include, a `#[path]`), to its package's root (a test's or build script's working
/// directory, `CARGO_MANIFEST_DIR`), or to the workspace's root (xtask's commands).
fn references(file: &RustSource, package_root: &Path, workspace_root: &Path) -> Vec<Reference> {
    let directory = file.path.parent().unwrap_or(package_root);
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let literals = string_literals(&file.code).into_iter().chain(string_literals(&file.docs));
    for (offset, literal) in literals.filter(|(_, literal)| path_shaped(literal)) {
        let below_root = literal.trim_start_matches('/');
        let candidates = [directory.join(literal), package_root.join(below_root), workspace_root.join(literal)]
            .map(|path| normalize(&path));
        for target in candidates {
            // `"/"`, `"a/.."`: a literal that names its own package or the workspace as a whole is data, not a read.
            let degenerate = target == package_root || target == workspace_root;
            let inside = target.starts_with(workspace_root) && !degenerate;
            let built = target.components().any(|part| matches!(part.as_os_str().to_str(), Some("target" | ".git")));
            let named = inside && !built && target != file.path && fs::symlink_metadata(&target).is_ok();
            if named && seen.insert(target.clone()) {
                found.push(Reference { offset, target });
            }
        }
    }
    found
}

/// Whether a literal can be a relative path naming something in another directory: it holds a `/`, and nothing a path
/// written whole never does.
fn path_shaped(literal: &str) -> bool {
    let unshaped = literal.chars().any(|character| character.is_whitespace() || matches!(character, '*' | '{' | '}'));
    literal.contains('/') && !unshaped && !literal.contains("://")
}

/// The names the build script in `package_root`, if it has one, sets for its package's compile with
/// `cargo::rustc-env=NAME=…`.
fn build_environment(package_root: &Path) -> BTreeSet<String> {
    let Some(script) = RustSource::read(&package_root.join("build.rs")) else {
        return BTreeSet::new();
    };
    string_literals(&script.code)
        .into_iter()
        .filter_map(|(_, literal)| literal.split_once("rustc-env=").and_then(|(_, set)| set.split_once('=')))
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// Every compile-time read in `code` whose file the references check cannot work out, with its offset and how it is
/// written.
///
/// Followed: a bare string literal; `concat!(env!("CARGO_MANIFEST_DIR"), "…")`; the package's own build output,
/// `env!("OUT_DIR")` joined or an `env!` its build script sets (`built`), whose own reads are the build script's, which
/// the check scans; and a proc macro's `#`-interpolated template, which names no file of its own.
fn include_arguments(code: &str, built: &BTreeSet<String>) -> Vec<(usize, String)> {
    let mut unfollowed = Vec::new();
    for (offset, form, argument) in path_arguments(code) {
        let trimmed = argument.trim();
        let literals = string_literals(trimmed);
        let first = literals.first().map_or("", |(_, literal)| *literal);
        let bare = trimmed.starts_with('"') || trimmed.starts_with("r\"") || trimmed.starts_with("r#");
        let interpolated = trimmed.starts_with('#');
        let manifest_relative = trimmed.starts_with("concat!") && trimmed.contains("env!(\"CARGO_MANIFEST_DIR\")");
        let out_dir = trimmed.starts_with("concat!") && trimmed.contains("env!(\"OUT_DIR\")");
        let set_by_build = trimmed.starts_with("env!(") && built.contains(first);
        let followed = bare || interpolated || manifest_relative || out_dir || set_by_build;
        if !followed {
            unfollowed.push((offset, format!("{form}({trimmed})")));
        }
    }
    unfollowed
}

/// Every `(offset, form, argument)` a compile-time path arrives through: the include macros, and `#[path = …]`.
fn path_arguments(code: &str) -> Vec<(usize, &'static str, &str)> {
    // A macro's or attribute's name inside a string literal is text, not a read.
    let quoted: Vec<(usize, usize)> =
        string_literals(code).into_iter().map(|(offset, literal)| (offset, offset + literal.len())).collect();
    let in_text = |at: usize| quoted.iter().any(|(start, end)| (*start..*end).contains(&at));
    let mut arguments = Vec::new();
    for name in INCLUDE_MACROS {
        let mut cursor = 0;
        while let Some(offset) = code[cursor..].find(name) {
            let start = cursor + offset;
            cursor = start + name.len();
            // `include!` also matches inside `include_str!`'s spelling of another macro; an identifier byte before
            // the match means it is another name's tail.
            let tail_of_another =
                code.as_bytes()[..start].last().is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
            if tail_of_another || in_text(start) {
                continue;
            }
            let Some(open) = code[cursor..].find('(').map(|offset| cursor + offset) else {
                continue;
            };
            let Some(close) = balanced_paren_end(code, open + 1) else {
                continue;
            };
            arguments.push((start, *name, &code[open + 1..close]));
        }
    }

    let mut cursor = 0;
    while let Some(offset) = code[cursor..].find("#[path") {
        let start = cursor + offset;
        cursor = start + "#[path".len();
        if in_text(start) {
            continue;
        }
        let Some(close) = code[start..].find(']').map(|offset| start + offset) else {
            continue;
        };
        let Some(equals) = code[start..close].find('=') else {
            continue;
        };
        arguments.push((start, "#[path]", &code[start + equals + 1..close]));
    }
    arguments.sort_by_key(|(offset, _, _)| *offset);
    arguments
}

/// The offset of the `)` closing the group whose body starts at `from`.
fn balanced_paren_end(code: &str, from: usize) -> Option<usize> {
    let mut depth = 1_usize;
    for (offset, byte) in code.bytes().enumerate().skip(from) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// `path` with `.` and `..` collapsed as text: the file it names need not exist, so `canonicalize` cannot.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    normalized
}
