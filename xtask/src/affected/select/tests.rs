//! What a change to the manifests or the lockfile selects, diffed between two commits of a [`Fixture`] workspace: the
//! graph at the base loaded from a worktree of it, as CI loads it.

use super::fixture::{Fixture, ROOT_MANIFEST};
use super::{Every, Selection};

/// The selection that names exactly `names`.
fn packages(names: &[&str]) -> Selection {
    Selection::Packages(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn a_bumped_crates_io_version_selects_its_dependents_and_what_builds_on_them() {
    // Catches a lockfile change sent to every package, the cost this replaces, and a version change the graph diff
    // misses, or finds only in the members that name the package and not in what builds on them (`beta`).
    let (fixture, base) = Fixture::new("version-bump");
    fixture.vendor("shared", "1.0.1");
    fixture.update("shared");
    fixture.commit();

    assert_eq!(fixture.select(&base, &["Cargo.lock"]), packages(&["alpha", "beta", "gamma", "xtask"]));
}

#[test]
fn an_edge_between_members_added_or_removed_selects_the_member_whose_edge_changed_and_its_dependents() {
    // Catches a removed edge marking the member it pointed at, which changed nothing, and an added edge missed in
    // what builds on the member that gained it (`epsilon` builds `delta`).
    let (fixture, base) = Fixture::new("internal-edge");
    fixture.member("beta", "");
    fixture.member("delta", r#"gamma = { path = "../gamma" }"#);
    fixture.lock();
    fixture.commit();

    let changed = ["crates/beta/Cargo.toml", "crates/delta/Cargo.toml", "Cargo.lock"];
    assert_eq!(fixture.select(&base, &changed), packages(&["beta", "delta", "epsilon", "xtask"]));
}

#[test]
fn a_new_crates_io_dependency_selects_the_member_that_gained_it_and_its_dependents() {
    // Catches the new package's arrival in the lockfile read as a change to every member, or to none.
    let (fixture, base) = Fixture::new("new-dependency");
    fixture.member("delta", r#"other = "1""#);
    fixture.lock();
    fixture.commit();

    let changed = ["crates/delta/Cargo.toml", "Cargo.lock"];
    assert_eq!(fixture.select(&base, &changed), packages(&["delta", "epsilon", "xtask"]));
}

#[test]
fn a_feature_one_member_turns_on_in_a_shared_dependency_selects_every_member_building_it() {
    // Catches feature unification left out: `gamma` turning on `shared/extra` changes `shared` in `alpha`'s build
    // under `cargo nextest run --workspace --all-features`, though no edge leads from `gamma` to `alpha` and the
    // lockfile, which records no features, does not change.
    let (fixture, base) = Fixture::new("unified-feature");
    fixture.member("gamma", r#"shared = { workspace = true, features = ["extra"] }"#);
    fixture.commit();

    assert_eq!(fixture.select(&base, &["crates/gamma/Cargo.toml"]), packages(&["alpha", "beta", "gamma", "xtask"]));
}

#[test]
fn a_lockfile_change_that_changes_no_graph_selects_nothing() {
    // Catches the lockfile still screened as shaping every build, and two equal graphs read as different, as they
    // would be if a package were compared by a path that differs between the base's worktree and the head's tree.
    let (fixture, base) = Fixture::new("no-graph-change");
    fixture.write("Cargo.lock", &fixture.read("Cargo.lock").replace("version = 4", "version = 3"));
    fixture.commit();

    assert_eq!(fixture.select(&base, &["Cargo.lock"]), Selection::Nothing);
}

#[test]
fn a_root_dependency_change_goes_to_the_graph_and_a_profile_change_selects_every_package() {
    // Catches `[workspace.dependencies]` sent to every package when the graph shows what it reaches, here every member
    // inheriting `shared` and not `delta`, and a profile, which the graph does not hold, sent to the graph and missed.
    let (fixture, base) = Fixture::new("root-manifest");
    let featured = ROOT_MANIFEST.replace(r#"shared = "1""#, r#"shared = { version = "1", features = ["extra"] }"#);
    fixture.write("Cargo.toml", &featured);
    let dependencies = fixture.commit();

    assert_eq!(fixture.select(&base, &["Cargo.toml"]), packages(&["alpha", "beta", "gamma", "xtask"]));

    fixture.write("Cargo.toml", &format!("{featured}\n[profile.dev]\nopt-level = 1\n"));
    fixture.commit();

    let profile = Selection::Every(Every::Section(String::from("profile")));
    assert_eq!(fixture.select(&dependencies, &["Cargo.toml"]), profile);
}

#[test]
fn a_base_whose_graph_cannot_be_loaded_selects_every_package() {
    // Catches a failed base read as an empty diff, which selects only what the paths reach: here `delta` alone, though
    // what its manifest's change reaches through the graph is unknown.
    let (fixture, _) = Fixture::new("unloadable-base");
    let manifest = fixture.read("crates/delta/Cargo.toml");
    fixture.write("crates/delta/Cargo.toml", "[package\n");
    let broken = fixture.commit();
    fixture.write("crates/delta/Cargo.toml", &manifest);
    fixture.commit();

    let selection = fixture.select(&broken, &["crates/delta/Cargo.toml"]);
    assert!(matches!(selection, Selection::Every(Every::Unknown(_))), "{selection:?}");
}
