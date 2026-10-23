//! One browser example's page, built into `pkg/` in its directory: the example for wasm32, the bindings the page
//! imports, and the page, made from `shared/index.html` with the example's own layout where it has one. A page is
//! served over http on localhost, where WebGPU is let run; `--serve` serves it.

mod page;
mod serve;

use crate::args::Serving;
use crate::cargo;
use crate::failure::Failure;
use std::fs;
use std::path::{Path, PathBuf};
use wasm_bindgen_cli_support::Bindgen;

/// The browser chassis's examples, from the workspace's root: a directory each, with its `lib.rs`.
const EXAMPLES: &str = "crates/aethereal-chassis-browser/examples";

/// Builds the example `name`'s page, prints `built example=<name> bytes=<module's bytes> page=<page, from the
/// workspace's root>`, and serves it if asked. A name that is not an example's is refused, listing the examples.
pub fn run(name: &str, serving: Serving) -> Result<(), Failure> {
    let examples = cargo::root().join(EXAMPLES);
    let names = names(&examples)?;
    let known = names.iter().any(|known| known == name);
    if !known {
        let listed = names.join(" ");
        return Err(Failure::usage(format_args!("usage: cargo xtask example <example>, one of: {listed}")));
    }

    cargo::run(
        cargo::command().args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--package",
            "aethereal-chassis-browser",
            "--example",
            name,
        ]),
        &format!("build={name}"),
    )?;

    let module = target_directory()?.join("wasm32-unknown-unknown/release/examples").join(format!("{name}.wasm"));
    let pkg = examples.join(name).join("pkg");
    Bindgen::new()
        .input_path(&module)
        .web(true)
        // The page calls `init()` with no module, which then fetches the one beside the bindings.
        .and_then(|bindgen| bindgen.typescript(false).omit_default_module_path(false).generate(&pkg))
        .map_err(|error| Failure::new(format_args!("failed bind={name} {error:#}")))?;
    page::write(&examples, name, &pkg)?;

    let bytes = fs::metadata(pkg.join(format!("{name}_bg.wasm")))?.len();
    println!("built example={name} bytes={bytes} page={EXAMPLES}/{name}/pkg/index.html");

    match serving {
        Serving::Not => Ok(()),
        Serving::At(port) => serve::serve(&pkg, name, port),
    }
}

/// The examples' names, sorted: each directory beside `shared/` that holds a `lib.rs`.
fn names(examples: &Path) -> Result<Vec<String>, Failure> {
    let mut names = Vec::new();
    for entry in fs::read_dir(examples)? {
        let path = entry?.path();
        let is_example = path.join("lib.rs").is_file();
        if is_example {
            names.extend(path.file_name().and_then(|name| name.to_str()).map(str::to_owned));
        }
    }
    names.sort();
    Ok(names)
}

/// Where cargo writes what it builds, as `cargo metadata` reports it.
fn target_directory() -> Result<PathBuf, Failure> {
    let output = cargo::command().args(["metadata", "--format-version", "1", "--no-deps"]).output()?;
    let failed = !output.status.success();
    if failed {
        return Err(Failure::new(String::from_utf8_lossy(&output.stderr)));
    }

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| Failure::new(format_args!("cargo metadata: {error}")))?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Failure::new("cargo metadata named no target_directory"))
}
