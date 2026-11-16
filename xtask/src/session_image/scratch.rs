//! The build's scratch directory: its context, a copy of the image's directory, and where what a channel serves is
//! written out.

use super::IMAGE;
use crate::cargo;
use crate::failure::Failure;
use std::fs;
use std::path::PathBuf;

/// The build's scratch directory, removed when dropped, so it goes whether the build succeeded or not.
pub struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// The scratch directory at `root`, its context holding each file of the image's directory. What a run killed
    /// before its drop left at `root` is removed first.
    pub fn write(root: PathBuf) -> Result<Self, Failure> {
        let _ = fs::remove_dir_all(&root);
        let scratch = Self { root };
        let context = scratch.context();
        fs::create_dir_all(&context)?;
        for entry in fs::read_dir(cargo::root().join(IMAGE))? {
            let entry = entry?;
            fs::copy(entry.path(), context.join(entry.file_name()))?;
        }

        Ok(scratch)
    }

    /// The build's context.
    pub fn context(&self) -> PathBuf {
        self.root.join("context")
    }

    /// Where the Dockerfile's `release` stage is written out: its `version` and `Packages` files.
    pub fn release(&self) -> PathBuf {
        self.root.join("release")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
