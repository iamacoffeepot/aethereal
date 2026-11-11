//! What a browser lists a namespace from: a manifest of its files, written when the page is built (ADR-0032,
//! ADR-0054).

use crate::{Namespace, Unlisted};
use aethereal_data::FilePath;
use std::str;

/// The files of a namespace a browser lists it from. A fetch cannot list, so they are written beside them when the page
/// is built, at [`Manifest::PATH`] beneath the namespace's URL, as text: each path on a line of its own. It holds what
/// the namespace's listing holds ([`Namespace::listing`]), so a browser lists the files a native host finds in the
/// namespace's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The paths it lists, sorted, each once.
    paths: Vec<FilePath>,
}

impl Manifest {
    /// Where a namespace's manifest is, within the namespace.
    pub const PATH: FilePath = FilePath::new("manifest");

    /// The manifest of `namespace` whose files are `paths`: what its listing holds of them.
    #[must_use]
    pub fn of(namespace: Namespace, paths: impl IntoIterator<Item = FilePath>) -> Self {
        Self { paths: namespace.listing(paths) }
    }

    /// The manifest of `namespace` written as `text`: what its listing holds of the paths `text` names.
    ///
    /// # Errors
    ///
    /// [`Unlisted::NotListed`] for a namespace that is not listed, and [`Unlisted::Failed`] for text that is not UTF-8
    /// or holds a line no path spells, as a page a server answers in place of a manifest does.
    pub fn read(namespace: Namespace, text: &[u8]) -> Result<Self, Unlisted> {
        if !namespace.is_listed() {
            return Err(Unlisted::NotListed);
        }
        let paths = str::from_utf8(text)
            .map_err(|_| Unlisted::Failed)?
            .lines()
            .map(FilePath::parse)
            .collect::<Result<Vec<FilePath>, _>>()
            .map_err(|_| Unlisted::Failed)?;

        Ok(Self::of(namespace, paths))
    }

    /// Its text: each path, and a line break after it.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for path in &self.paths {
            text.push_str(path.as_str());
            text.push('\n');
        }

        text
    }

    /// The paths it lists, sorted, each once.
    #[must_use]
    pub fn into_paths(self) -> Vec<FilePath> {
        self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::Manifest;
    use crate::{Namespace, Unlisted};
    use aethereal_data::FilePath;

    #[test]
    fn a_manifest_holds_the_modules_it_is_written_with_and_reads_back_from_its_text() {
        // Catches a page's build and a browser that disagree on the text, a manifest listing what a native host would
        // not (a module beneath a directory, a file that is no module), and a listing in the order it was found.
        let found = ["b.wasm", "notes.txt", "deps/c.wasm", "a.wasm", "a.wasm"].map(FilePath::new);

        let manifest = Manifest::of(Namespace::Code, found);

        assert_eq!(manifest.text(), "a.wasm\nb.wasm\n");
        assert_eq!(Manifest::read(Namespace::Code, manifest.text().as_bytes()), Ok(manifest.clone()));
        assert_eq!(manifest.into_paths(), vec![FilePath::new("a.wasm"), FilePath::new("b.wasm")]);
        let written_by_hand = Manifest::read(Namespace::Code, b"z.wasm\nreadme.txt\nlib/y.wasm\nx.wasm");
        assert_eq!(
            written_by_hand.map(Manifest::into_paths),
            Ok(vec![FilePath::new("x.wasm"), FilePath::new("z.wasm")])
        );
    }

    #[test]
    fn text_that_is_no_manifest_fails_and_a_namespace_not_listed_is_refused() {
        // Catches a page a server answers in place of a missing manifest read as a listing of nothing, and a namespace
        // that is not listed listed from a manifest someone put in it.
        assert_eq!(Manifest::read(Namespace::Code, b"<!doctype html>\n<html>\n"), Err(Unlisted::Failed));
        assert_eq!(Manifest::read(Namespace::Code, &[0xFF, b'\n']), Err(Unlisted::Failed));
        assert_eq!(Manifest::read(Namespace::Assets, b"a.wasm\n"), Err(Unlisted::NotListed));

        assert_eq!(Manifest::read(Namespace::Code, b"").map(Manifest::into_paths), Ok(Vec::new()));
    }
}
