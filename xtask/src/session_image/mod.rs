//! The image Claude sessions run in (ADR-0057), made from `crates/aethereal-sessions/image` when the owner chooses:
//! Claude Code at an exact version, installed from Anthropic's apt repository, which apt verifies against the
//! release key committed beside the Dockerfile, on the Rust base image pinned by its digest. The hub never builds
//! one; its `sessions.image` setting names the tag this prints.
//!
//! The build runs on the machine's Docker through its CLI, on `BuildKit`: what a channel serves is read by building
//! the Dockerfile's `release` stage out to files, and the image is built from a context written under the target
//! directory, removed whether or not the build succeeds.

mod claude;
mod pins;
mod scratch;
mod site;
#[cfg(test)]
mod tests;

pub use claude::Claude;

use crate::cargo;
use crate::failure::Failure;
use claude::Resolved;
use scratch::Scratch;
use site::Site;
use std::io;
use std::process::{self, Command, Stdio};

/// The image's directory, from the workspace's root: its Dockerfile, Anthropic's release key it trusts, and the prompt
/// each session is told.
const IMAGE: &str = "crates/aethereal-sessions/image";

/// The repository's toolchain, whose version the image is built on unless `--rust` names another.
const TOOLCHAIN: &str = include_str!("../../../rust-toolchain.toml");

/// The uid and gid the session's user is built with unless `--uid` or `--gid` names another: the hub's
/// `sessions.user` default.
pub const DEFAULT_ID: u32 = 1000;

/// What the command line asks the image be made of.
pub struct Request {
    pub claude: Claude,
    pub uid: u32,
    pub gid: u32,
    pub base: Base,
    pub tag: Tag,
}

/// The tag the image is given.
pub enum Tag {
    /// `aethereal-session:cc-<version>`, for the version of Claude Code it holds.
    Versioned,
    /// The tag `--tag` names.
    Named(String),
}

/// The Rust base image the session image is built on: a Rust version whose image's digest is pinned.
#[derive(Debug)]
pub struct Base {
    rust: String,
    digest: &'static str,
}

impl Base {
    /// The base of Rust `rust`, refused unless its image's digest is pinned.
    pub fn pinned(rust: &str) -> Result<Self, Failure> {
        let (_, digest) = pins::RUST_IMAGES.iter().find(|(pinned, _)| *pinned == rust).ok_or_else(|| {
            Failure::usage(format_args!(
                "refused rust={rust}: no digest of rust:{rust}-slim is pinned in xtask/src/session_image/pins.rs"
            ))
        })?;

        Ok(Self { rust: String::from(rust), digest })
    }

    /// The base of the repository's toolchain, the channel `rust-toolchain.toml` names.
    pub fn toolchain() -> Result<Self, Failure> {
        let channel =
            TOOLCHAIN.lines().find_map(|line| line.strip_prefix("channel = ")).map(|text| text.trim_matches('"'));
        let rust = channel.ok_or_else(|| Failure::new("rust-toolchain.toml names no channel"))?;

        Self::pinned(rust)
    }

    /// The image, by its tag and its digest: `rust:<version>-slim@sha256:…`.
    fn image(&self) -> String {
        format!("rust:{}-slim@{}", self.rust, self.digest)
    }
}

/// One build of the image: its tag, and the build arguments its Dockerfile is given.
struct Build {
    tag: String,
    args: Vec<(&'static str, String)>,
}

impl Request {
    /// The build of the image holding Claude Code as `resolved`.
    fn build(&self, resolved: Resolved) -> Build {
        let Resolved { version, channel } = resolved;
        let tag = match &self.tag {
            Tag::Versioned => format!("aethereal-session:cc-{version}"),
            Tag::Named(tag) => tag.clone(),
        };
        let args = vec![
            ("BASE_IMAGE", self.base.image()),
            ("CLAUDE_CHANNEL", String::from(channel.name())),
            ("CLAUDE_VERSION", version.to_string()),
            ("CLAUDE_RELEASE_FINGERPRINT", String::from(pins::CLAUDE_RELEASE_FINGERPRINT)),
            ("SESSION_UID", self.uid.to_string()),
            ("SESSION_GID", self.gid.to_string()),
        ];

        Build { tag, args }
    }
}

/// Makes the image `request` asks for: resolves its Claude Code's version, refused unless its repository lists it,
/// and prints `claude=<version>`, with `channel=<name>` where a channel named it; builds the image, Docker's output on
/// stderr; and prints `image=<tag>`.
pub fn run(request: &Request) -> Result<(), Failure> {
    let root = cargo::target_directory()?.join("session-image").join(process::id().to_string());
    let scratch = Scratch::write(root)?;
    let site = Site { scratch: &scratch, base: request.base.image() };
    let resolved = request.claude.resolve(&site)?;
    println!("{}", request.claude.printed(resolved));

    let build = request.build(resolved);
    let mut command = docker_build();
    command.args(["--tag", &build.tag]);
    for (name, value) in &build.args {
        command.args(["--build-arg", &format!("{name}={value}")]);
    }
    cargo::run(command.arg(scratch.context()), "build=session-image")?;
    println!("image={}", build.tag);

    Ok(())
}

/// `docker build`, `BuildKit` forced on, its output on this process's stderr, so stdout holds only what this command prints.
fn docker_build() -> Command {
    let mut command = Command::new("docker");
    command.arg("build").env("DOCKER_BUILDKIT", "1").stdout(Stdio::from(io::stderr()));
    command
}
