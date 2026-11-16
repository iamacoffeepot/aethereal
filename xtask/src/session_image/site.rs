//! Anthropic's release site, read for what a channel serves through the build's own Docker, so the machine needs no
//! client of its own.

use super::claude::{Channel, Releases, Served};
use super::docker_build;
use super::scratch::Scratch;
use crate::cargo;
use crate::failure::Failure;
use std::fs;

/// The release site, reached by building the Dockerfile's `release` stage, which fetches the channel's version and
/// its repository's index for the build's architecture, out to `scratch`.
pub struct Site<'a> {
    pub scratch: &'a Scratch,
    /// The base image, which the Dockerfile's other stages name, so the build reads it.
    pub base: String,
}

impl Releases for Site<'_> {
    fn served(&self, channel: Channel) -> Result<Served, Failure> {
        let name = channel.name();
        let out = self.scratch.release();
        let mut command = docker_build();
        // The stage's steps are its fetches, and a cached one would be what the channel served an earlier day.
        command.args(["--no-cache", "--target", "release"]);
        command.args(["--build-arg", &format!("CLAUDE_CHANNEL={name}")]);
        command.args(["--build-arg", &format!("BASE_IMAGE={}", self.base)]);
        command.args(["--output", &format!("type=local,dest={}", out.display())]);
        cargo::run(command.arg(self.scratch.context()), &format!("release={name}"))?;

        Ok(Served {
            version: fs::read_to_string(out.join("version"))?,
            packages: fs::read_to_string(out.join("Packages"))?,
        })
    }
}
