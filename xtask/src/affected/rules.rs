//! What selects every package before the graph is read, and how determinator reads the paths that do not: the files that
//! shape every build or test, and the ones that reach none.

use determinator::rules::{DeterminatorMarkChanged, DeterminatorPostRule, DeterminatorRules, PathRule};
use guppy::graph::PackageGraph;

/// Paths whose change can reach every package's build or tests: the package graph, the toolchain, cargo's and
/// nextest's configuration, CI's workflow, and the parts of xtask that run the suite or parse what it runs. Any one
/// selects every package before the graph is read.
///
/// That is also what lets [`super::select::select`] hand determinator one graph as both the old and the new: a change
/// that could reshape the graph never reaches it, as a member's dependency edited always changes `Cargo.lock`.
///
/// `clippy.toml` configures the toolchain's lints rather than a test, and is here with the rest of the toolchain's
/// configuration: it changes rarely, and a run of everything when it does costs minutes. A change elsewhere in xtask
/// selects xtask through the graph, as any package's does.
const EVERY_EXACT: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "clippy.toml",
    ".github/workflows/ci.yml",
    "xtask/Cargo.toml",
    "xtask/src/main.rs",
    "xtask/src/args.rs",
    "xtask/src/cargo.rs",
    "xtask/src/check.rs",
    "xtask/src/failure.rs",
];

/// Directories whose every file selects every package, as [`EVERY_EXACT`]'s do: cargo's configuration, nextest's, and
/// the module that computes the selection.
const EVERY_PREFIX: &[&str] = &[".cargo/", ".config/", "xtask/src/affected/"];

/// Paths that reach no build and no test, so a change to them selects nothing: documents, which CI's own `paths-ignore`
/// already skips when they are all a pull request changes (`docs/**`, `**.md`), the agents' settings and hooks, and the
/// workflows other than `ci.yml`, which [`EVERY_EXACT`] holds.
///
/// They are applied before determinator's defaults, which also ignore `README*`, `LICENSE*`, `.gitignore`,
/// `rustfmt.toml`, and the rest of its list. A file one of these claims that a build or a test does read is caught by
/// the `references` invariant, which fails where a change to a file read selects less than a change to its reader.
const IGNORED: &[&str] = &["docs/**", "**/*.md", ".claude/**", ".github/**"];

/// The first changed path that selects every package, where one does.
pub fn screen(changed: &[String]) -> Option<&str> {
    changed
        .iter()
        .map(String::as_str)
        .find(|path| EVERY_EXACT.contains(path) || EVERY_PREFIX.iter().any(|prefix| path.starts_with(prefix)))
}

/// The path rules determinator applies before its defaults and before it maps a path to its nearest package: the
/// [`IGNORED`] paths, then one rule for each member inside another member's directory, marking the enclosing member
/// changed as well as the nested one.
///
/// A member nested in another is built by it, or may be: the headless chassis's build script builds its guests
/// (`guests/echo`, a member cargo resolves as a path dev-dependency) for wasm32, on every build of the chassis, its
/// dependents' builds included. A change to a guest can fail that build, so it selects the chassis and every package
/// building on it, not the chassis's tests alone, which is all the guest's own dev-dependency edge reaches. The
/// `nesting` invariant checks the rest: that the members a guest is built from reach as far.
pub fn determinator_rules(graph: &PackageGraph) -> DeterminatorRules {
    let mut rules = DeterminatorRules::default();
    rules.path_rules.push(PathRule {
        globs: IGNORED.iter().map(|glob| (*glob).to_owned()).collect(),
        mark_changed: DeterminatorMarkChanged::Packages(Vec::new()),
        post_rule: DeterminatorPostRule::Skip,
    });

    let roots: Vec<(&str, &str)> = graph
        .workspace()
        .iter()
        .filter_map(|package| Some((package.source().workspace_path()?.as_str(), package.name())))
        .filter(|(root, _)| !root.is_empty())
        .collect();
    for (inner, _) in &roots {
        let enclosing: Vec<String> = roots
            .iter()
            .filter(|(outer, _)| inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/')))
            .map(|(_, name)| (*name).to_owned())
            .collect();
        if !enclosing.is_empty() {
            rules.path_rules.push(PathRule {
                globs: vec![format!("{inner}/**")],
                mark_changed: DeterminatorMarkChanged::Packages(enclosing),
                post_rule: DeterminatorPostRule::Fallthrough,
            });
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::screen;

    #[test]
    fn the_screen_holds_what_shapes_every_build_and_nothing_else() {
        // Catches an entry dropped from the screen, which lets a change to the graph, the toolchain, or what runs the
        // suite run a stale subset, and one widened past it, which runs everything for a crate's own source.
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "clippy.toml",
            ".cargo/config.toml",
            ".config/nextest.toml",
            ".github/workflows/ci.yml",
            "xtask/Cargo.toml",
            "xtask/src/main.rs",
            "xtask/src/args.rs",
            "xtask/src/cargo.rs",
            "xtask/src/check.rs",
            "xtask/src/affected/rules.rs",
        ] {
            assert_eq!(screen(&[path.to_owned()]), Some(path), "{path} selects every package");
        }

        for path in [
            "crates/aethereal-ui/src/lib.rs",
            "crates/aethereal-ui/Cargo.toml",
            "crates/aethereal-chassis-headless/guests/echo/Cargo.lock",
            "xtask/src/session_image/pins.rs",
            ".github/workflows/lint-title.yml",
        ] {
            assert_eq!(screen(&[path.to_owned()]), None, "{path} is left to the graph");
        }
    }
}
