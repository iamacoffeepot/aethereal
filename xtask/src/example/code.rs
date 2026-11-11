//! The manifest of a page's code: the modules in its `code/`, which a browser lists the namespace `Code` from, as a
//! fetch cannot list (ADR-0032, ADR-0054).

use crate::failure::Failure;
use aethereal_data::FilePath;
use aethereal_files::{Manifest, Namespace};
use std::fs;
use std::path::Path;

/// The directory beside the page its code is fetched beneath: the browser's `files.code` as it is set by default.
const CODE: &str = "code";

/// Writes the manifest of the code in `pkg`, the modules directly in its `code/`, as the browser reads it. A page with
/// no `code/` is given none, so its `Code` is answered missing, as a native host's with no directory is.
pub fn write(pkg: &Path) -> Result<(), Failure> {
    let code = pkg.join(CODE);
    if !code.is_dir() {
        return Ok(());
    }
    let manifest = Manifest::of(Namespace::Code, found(&code)?);
    fs::write(code.join(Manifest::PATH.as_str()), manifest.text())?;

    Ok(())
}

/// The files directly in `directory`, each a file or a link to one, whose names a path spells, as a native host's
/// storage finds them: the manifest holds what the namespace's listing holds of them.
fn found(directory: &Path) -> Result<Vec<FilePath>, Failure> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(path) = FilePath::parse(name) else {
            continue;
        };
        if entry.path().is_file() {
            found.push(path);
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::write;
    use std::path::PathBuf;
    use std::{env, fs, process};

    /// A page's directory, named for `test`, removed when dropped.
    struct Pkg(PathBuf);

    impl Pkg {
        /// A page's directory holding `files`, each a path beneath it and its text.
        fn new(test: &str, files: &[(&str, &str)]) -> Self {
            let root = env::temp_dir().join(format!("aethereal-xtask-code-{}-{test}", process::id()));
            let _ = fs::remove_dir_all(&root);
            let made = fs::create_dir_all(&root);
            assert!(made.is_ok(), "the page's directory is made");
            for (path, text) in files {
                let file = root.join(path);
                let made = file.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&file, text));
                assert!(made.is_ok(), "the test's file `{path}` is written");
            }

            Self(root)
        }

        /// Writes the manifest of its code.
        fn write(&self) {
            let Ok(()) = write(&self.0) else {
                panic!("the manifest is written");
            };
        }

        /// The path beneath it `path` names.
        fn at(&self, path: &str) -> PathBuf {
            self.0.join(path)
        }
    }

    impl Drop for Pkg {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_page_s_code_is_listed_by_its_modules_and_a_page_with_none_gets_no_manifest() {
        // Catches a manifest of the wrong directory, one of the whole tree or of files that are no module, which a
        // browser would list where a native host would not, and a `code/` made for a page that has none.
        let built = Pkg::new(
            "built",
            &[
                ("code/b.wasm", "b"),
                ("code/a.wasm", "a"),
                ("code/a.d", "a"),
                ("code/deps/c.wasm", "c"),
                ("x.wasm", "x"),
            ],
        );
        let bare = Pkg::new("bare", &[("index.html", "page")]);

        built.write();
        bare.write();

        assert_eq!(fs::read_to_string(built.at("code/manifest")).ok().as_deref(), Some("a.wasm\nb.wasm\n"));
        assert!(!bare.at("code").exists(), "a page with no code is given none");
    }
}
