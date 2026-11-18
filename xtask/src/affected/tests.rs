use super::changed_paths;
use super::graph::Workspace;
use super::select::{Every, Selection};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{self, Command};
use std::{env, fs};

/// Runs `git <args>` in `directory`, answering with what it printed.
fn git_in(directory: &Path, args: &[&str]) -> String {
    let Ok(output) = Command::new("git").args(args).current_dir(directory).output() else {
        panic!("git runs");
    };
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_file_moved_between_packages_changes_both() {
    // Catches git's rename detection, which lists a moved file under the path it reached alone: a file moved out of a
    // package would leave that package unselected, though it lost the file.
    let repository = env::temp_dir().join(format!("aethereal-xtask-affected-{}", process::id()));
    let _ = fs::remove_dir_all(&repository);
    let source = repository.join("crates/one/src");
    let destination = repository.join("crates/two/src");
    for directory in [&source, &destination] {
        assert!(fs::create_dir_all(directory).is_ok());
    }
    assert!(fs::write(source.join("moved.rs"), "pub fn moved() {}\n".repeat(20)).is_ok());
    git_in(&repository, &["init", "--quiet"]);
    git_in(&repository, &["add", "--all"]);
    let before = git_in(&repository, &["write-tree"]);

    assert!(fs::rename(source.join("moved.rs"), destination.join("moved.rs")).is_ok());
    git_in(&repository, &["add", "--all"]);
    let after = git_in(&repository, &["write-tree"]);
    let changed = changed_paths(&repository, &before, &after);
    let _ = fs::remove_dir_all(&repository);

    let Ok(changed) = changed else {
        panic!("the trees diff");
    };
    assert_eq!(changed, ["crates/one/src/moved.rs", "crates/two/src/moved.rs"]);
}

/// What a change to `changed` selects in this workspace.
fn selected(workspace: &Workspace, changed: &[&str]) -> Selection {
    let changed: Vec<String> = changed.iter().map(|path| (*path).to_owned()).collect();
    let Ok(selection) = workspace.select_paths(&changed) else {
        panic!("a selection is computed for {changed:?}");
    };
    selection
}

/// The packages a selection names, where it names some.
fn packages(selection: Selection) -> BTreeSet<String> {
    let Selection::Packages(packages) = selection else {
        panic!("a narrowed selection, not {selection:?}");
    };
    packages
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn loaded() -> Workspace {
    let Ok(workspace) = Workspace::load() else {
        panic!("the package graph loads");
    };
    workspace
}

#[test]
fn a_crate_s_change_selects_it_and_what_builds_on_it_and_not_what_it_builds_on() {
    // Catches an inverted closure, which selects aethereal-ui's own dependencies and misses its dependents, and one
    // that drops dev edges, which misses aethereal-ui-macros, whose tests build aethereal-ui. xtask, whose invariants
    // scan every package, comes with any package.
    let ui = packages(selected(&loaded(), &["crates/aethereal-ui/src/lib.rs"]));

    assert!(ui.is_superset(&names(&["aethereal-ui", "aethereal-overlay", "aethereal-ui-macros", "xtask"])), "{ui:?}");
    assert!(ui.is_disjoint(&names(&["aethereal-render", "aethereal-math", "aethereal-data"])), "{ui:?}");
}

#[test]
fn a_change_to_a_package_s_own_tests_selects_that_package_alone() {
    // Catches the narrowing lost, which runs every dependent's tests for an integration test nothing links, and a
    // narrowing that swallows a library change in the same package, which misses every dependent.
    let workspace = loaded();
    let tests_only = packages(selected(&workspace, &["crates/aethereal-chassis-headless/tests/library.rs"]));
    let library = packages(selected(&workspace, &["crates/aethereal-chassis-headless/src/lib.rs"]));

    assert_eq!(tests_only, names(&["aethereal-chassis-headless", "xtask"]));
    assert!(library.is_superset(&names(&["aethereal-chassis-desktop", "aethereal-overlay"])), "{library:?}");
}

#[test]
fn a_path_that_shapes_every_build_selects_every_package_and_documents_select_none() {
    // Catches the screen bypassed, which hands a toolchain or CI change to the graph, and documents that select a
    // package, which runs tests CI's own `paths-ignore` would have skipped.
    let workspace = loaded();
    let workflow = ".github/workflows/ci.yml";

    assert_eq!(selected(&workspace, &[workflow]), Selection::Every(Every::Shaping(workflow.to_owned())));
    assert_eq!(
        selected(&workspace, &["docs/adr/0001-entry.md", "crates/aethereal-render/fonts/README.md"]),
        Selection::Nothing
    );
    assert_eq!(
        selected(&workspace, &["docs/adr/0001-entry.md", "crates/aethereal-ui/src/lib.rs"]),
        selected(&workspace, &["crates/aethereal-ui/src/lib.rs"]),
        "a document beside a crate's change adds nothing to it"
    );
}

#[test]
fn a_path_no_package_or_rule_claims_selects_every_package() {
    // Catches an unknown input deselected silently: the one failure this tool must never have.
    assert_eq!(selected(&loaded(), &["mystery-toplevel-input.txt"]), Selection::Every(Every::Reached));
}

#[test]
fn a_file_read_from_another_package_selects_its_reader() {
    // Catches each coupling the graph alone would miss: aethereal-ui's fonts, read from aethereal-render's directory
    // by `include_bytes!`; the headless chassis's guest, which its build script builds on every build of the chassis,
    // dependents' included; and the session image's directory and the browser examples' page, which xtask reads.
    let workspace = loaded();
    let font = packages(selected(&workspace, &["crates/aethereal-render/fonts/IBMPlexSans-Regular.ttf"]));
    let guest = packages(selected(&workspace, &["crates/aethereal-chassis-headless/guests/echo/src/lib.rs"]));
    let image = packages(selected(&workspace, &["crates/aethereal-sessions/image/Dockerfile"]));
    let page = packages(selected(&workspace, &["crates/aethereal-chassis-browser/examples/shared/index.html"]));

    assert!(font.contains("aethereal-ui"), "{font:?}");
    let builders = names(&["aethereal-chassis-headless", "aethereal-chassis-desktop", "aethereal-overlay"]);
    assert!(guest.is_superset(&builders), "{guest:?}");
    assert!(image.contains("xtask"), "{image:?}");
    assert!(page.contains("xtask"), "{page:?}");
}
