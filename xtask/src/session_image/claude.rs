//! Which Claude Code a session image holds: a version named, or a release channel's, resolved to its version, and
//! either refused unless the channel's apt repository lists it, before the build.

use crate::failure::Failure;
use std::fmt;

/// A Claude Code release's version, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// The version `text` writes, three numbers in decimal digits apart by dots, and nothing else.
    pub fn read(text: &str) -> Option<Self> {
        let mut parts = text.split('.').map(number);
        let (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };

        Some(Self { major, minor, patch })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The number one part of a version writes, in decimal digits alone.
fn number(part: &str) -> Option<u32> {
    let digits = part.bytes().all(|byte| byte.is_ascii_digit());
    if digits {
        part.parse().ok()
    } else {
        None
    }
}

/// A release channel: the version Anthropic's release site names for it, and the apt repository of its versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Every release, and so the repository with the most history: a version named is installed from it.
    Latest,
    Stable,
}

impl Channel {
    /// The channel's name, as the release site, its repository, and the command line write it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Stable => "stable",
        }
    }
}

/// What a channel serves, as text: the version its release names, and its apt repository's `Packages` index.
pub struct Served {
    pub version: String,
    pub packages: String,
}

/// Where a channel's release is read: Anthropic's release site in a build, the test's answer in a test.
pub trait Releases {
    /// What `channel` serves.
    fn served(&self, channel: Channel) -> Result<Served, Failure>;
}

/// The Claude Code the command line asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claude {
    /// A version named, installed from the `latest` repository.
    Version(Version),
    /// A channel's version, whichever it is when the image is made, installed from the channel's repository.
    Channel(Channel),
}

/// The Claude Code an image is built with: the exact version, and the channel whose repository it is installed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub version: Version,
    pub channel: Channel,
}

impl Claude {
    /// What `--claude` names: a version, or `latest` or `stable`.
    pub fn read(text: &str) -> Result<Self, Failure> {
        match text {
            "latest" => Ok(Self::Channel(Channel::Latest)),
            "stable" => Ok(Self::Channel(Channel::Stable)),
            _ => Version::read(text).map(Self::Version).ok_or_else(|| {
                Failure::usage(format_args!(
                    "refused claude={text}: --claude takes a version, major.minor.patch, or latest or stable"
                ))
            }),
        }
    }

    /// The version the image holds, and its channel: the version named, or the one `releases` names for the channel,
    /// refused unless the channel's repository lists its package.
    pub fn resolve(self, releases: &impl Releases) -> Result<Resolved, Failure> {
        let channel = match self {
            Self::Version(_) => Channel::Latest,
            Self::Channel(channel) => channel,
        };
        let name = channel.name();
        let served = releases.served(channel)?;
        let version = match self {
            Self::Version(version) => version,
            Self::Channel(_) => Version::read(served.version.trim())
                .ok_or_else(|| Failure::new(format_args!("refused channel={name}: its release names no version")))?,
        };

        let listed = served.packages.split("\n\n").any(|stanza| lists(stanza, version));
        if !listed {
            return Err(Failure::new(format_args!(
                "refused claude={version}: the {name} apt repository lists no claude-code {version}-1"
            )));
        }

        Ok(Resolved { version, channel })
    }

    /// The line printed for the image's Claude Code: `claude=<version>`, and the channel it was resolved from, where
    /// one named it, as `channel=<name>`.
    pub fn printed(self, resolved: Resolved) -> String {
        let version = resolved.version;
        match self {
            Self::Version(_) => format!("claude={version}"),
            Self::Channel(channel) => format!("claude={version} channel={}", channel.name()),
        }
    }
}

/// Whether `stanza` of a `Packages` index is claude-code's package of `version`, the Debian revision `-1`.
fn lists(stanza: &str, version: Version) -> bool {
    let wanted = format!("Version: {version}-1");
    let mut fields = stanza.lines();
    let named = fields.clone().any(|line| line == "Package: claude-code");
    let versioned = fields.any(|line| line == wanted);

    named && versioned
}
