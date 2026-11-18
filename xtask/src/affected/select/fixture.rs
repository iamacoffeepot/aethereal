//! A small workspace in a git repository of its own, for the selection to diff two of its commits.
//!
//! Its crates.io packages come from a directory of vendored ones outside the repository, which its `.cargo/config.toml`
//! replaces crates.io with: cargo resolves and names them as crates.io packages, with no network, and a version vendored
//! after a commit leaves that commit's lockfile resolvable.

use crate::affected::base::BaseTree;
use crate::affected::graph::Workspace;
use crate::affected::select::Selection;
use crate::cargo;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::{env, fs};

/// The members every fixture starts with, each with its `[dependencies]`: `alpha` and `gamma` build `shared` from
/// crates.io, `beta` builds `alpha`, `epsilon` builds `delta`, and `xtask` stands for the workspace scanner every
/// narrowed selection holds.
const MEMBERS: &[(&str, &str)] = &[
    ("alpha", "shared = { workspace = true }"),
    ("beta", r#"alpha = { path = "../alpha" }"#),
    ("gamma", "shared = { workspace = true }"),
    ("delta", ""),
    ("epsilon", r#"delta = { path = "../delta" }"#),
    ("xtask", ""),
];

/// The root manifest every fixture starts with.
pub const ROOT_MANIFEST: &str = r#"[workspace]
resolver = "3"
members = ["crates/alpha", "crates/beta", "crates/gamma", "crates/delta", "crates/epsilon", "crates/xtask"]

[workspace.dependencies]
shared = "1"
"#;

/// A workspace in a repository under the system's temporary directory, removed when dropped.
pub struct Fixture {
    directory: PathBuf,
    repository: PathBuf,
    vendor: PathBuf,
}

impl Fixture {
    /// A workspace of [`MEMBERS`] under [`ROOT_MANIFEST`], with `shared` 1.0.0 (feature `extra`) and `other` 1.0.0
    /// vendored, locked and committed: the commit returned.
    pub fn new(name: &str) -> (Self, String) {
        let directory = env::temp_dir().join(format!("aethereal-xtask-affected-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&directory);
        let fixture = Self { repository: directory.join("repository"), vendor: directory.join("vendor"), directory };

        let source = format!(
            "[source.crates-io]\nreplace-with = \"fixture\"\n\n[source.fixture]\ndirectory = \"{}\"\n",
            fixture.vendor.display()
        );
        fixture.write(".cargo/config.toml", &source);
        fixture.vendor("shared", "1.0.0");
        fixture.vendor("other", "1.0.0");
        fixture.write("Cargo.toml", ROOT_MANIFEST);
        for (member, dependencies) in MEMBERS {
            fixture.member(member, dependencies);
        }
        fixture.git(&["init", "--quiet"]);

        fixture.lock();
        let base = fixture.commit();
        (fixture, base)
    }

    /// Vendors crates.io's `name` at `version`, with an empty library and one feature, `extra`.
    pub fn vendor(&self, name: &str, version: &str) {
        let crate_directory = self.vendor.join(format!("{name}-{version}"));
        let manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2024\"\n\n[features]\nextra = []\n"
        );
        let checksum = format!("{{\"files\":{{}},\"package\":\"{:064}\"}}", 1);
        for (path, text) in
            [("Cargo.toml", manifest.as_str()), ("src/lib.rs", ""), (".cargo-checksum.json", checksum.as_str())]
        {
            write(&crate_directory.join(path), text);
        }
    }

    /// Writes member `name`'s manifest under `crates/`, with `dependencies` as its `[dependencies]`, and its library.
    pub fn member(&self, name: &str, dependencies: &str) {
        let manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}\n"
        );
        self.write(&format!("crates/{name}/Cargo.toml"), &manifest);
        self.write(&format!("crates/{name}/src/lib.rs"), "");
    }

    /// Writes `text` to `path`, relative to the repository's root.
    pub fn write(&self, path: &str, text: &str) {
        write(&self.repository.join(path), text);
    }

    /// The file at `path`, relative to the repository's root.
    pub fn read(&self, path: &str) -> String {
        let Ok(text) = fs::read_to_string(self.repository.join(path)) else {
            panic!("{path} reads");
        };
        text
    }

    /// Resolves the lockfile anew from the manifests, as a change to them does.
    pub fn lock(&self) {
        self.cargo(&["metadata", "--format-version", "1", "--offline"]);
    }

    /// Moves `package` to the newest version vendored, as `cargo update` does.
    pub fn update(&self, package: &str) {
        self.cargo(&["update", "--offline", "-p", package]);
    }

    /// Commits the whole tree, answering with the commit.
    pub fn commit(&self) -> String {
        self.git(&["add", "--all"]);
        self.git(&["-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// What a change from `base` to the tree as it stands, changing `changed`, selects.
    pub fn select(&self, base: &str, changed: &[&str]) -> Selection {
        let Ok(workspace) = Workspace::load_at(&self.repository) else {
            panic!("the head's package graph loads");
        };
        let changed: Vec<String> = changed.iter().map(|path| (*path).to_owned()).collect();
        let Ok(selection) = workspace.select(&changed, &BaseTree::new(&self.repository, base)) else {
            panic!("a selection is computed for {changed:?}");
        };
        selection
    }

    fn cargo(&self, args: &[&str]) {
        let Ok(output) = cargo::command().args(args).current_dir(&self.repository).output() else {
            panic!("cargo runs");
        };
        assert!(output.status.success(), "cargo {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Runs `git <args>` in the repository, as a fixed author, answering with what it printed.
    fn git(&self, args: &[&str]) -> String {
        let Ok(output) = Command::new("git")
            .args(args)
            .current_dir(&self.repository)
            .envs([("GIT_AUTHOR_NAME", "fixture"), ("GIT_COMMITTER_NAME", "fixture")])
            .envs([("GIT_AUTHOR_EMAIL", "fixture@example.invalid"), ("GIT_COMMITTER_EMAIL", "fixture@example.invalid")])
            .output()
        else {
            panic!("git runs");
        };
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Writes `text` to `path`, making its directory.
fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        assert!(fs::create_dir_all(parent).is_ok(), "{} is made", parent.display());
    }
    assert!(fs::write(path, text).is_ok(), "{} is written", path.display());
}
