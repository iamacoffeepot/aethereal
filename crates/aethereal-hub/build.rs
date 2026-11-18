//! Reads the commit the hub is built from, which `describe_hub` answers: empty when git reads none, as in a build from
//! a source tarball or with no git installed.

use std::path::Path;
use std::process::Command;

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    println!("cargo::rustc-env=AETHEREAL_BUILD_COMMIT={commit}");

    // The commit changes only when `HEAD` moves: a checkout moves `HEAD` itself, and a commit moves the branch it names,
    // kept loose or packed. Only a file that exists is named, as cargo runs a script again on every build while a file
    // it names is missing. With none named, cargo would run it again on any change to the package.
    println!("cargo::rerun-if-changed=build.rs");
    let branch = git(&["symbolic-ref", "-q", "HEAD"]);
    let watched = ["HEAD", "packed-refs"]
        .into_iter()
        .map(String::from)
        .chain(branch)
        .filter_map(|name| git(&["rev-parse", "--path-format=absolute", "--git-path", &name]))
        .filter(|path| Path::new(path).exists());
    for path in watched {
        println!("cargo::rerun-if-changed={path}");
    }
}

/// What `git` with `args` prints, trimmed: none when it cannot run or fails, as where the source is no git checkout.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok().map(|text| text.trim().to_owned())
}
