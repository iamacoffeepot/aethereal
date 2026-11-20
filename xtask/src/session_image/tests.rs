use super::claude::{Channel, Claude, Releases, Resolved, Served};
use super::scratch::Scratch;
use super::{Base, Request, pins};
use crate::args::{self, Command};
use crate::failure::Failure;
use std::{env, iter, process};

/// A `Packages` index listing claude-code 2.1.310-1 and 2.1.325-1, and another package at 2.1.200-1.
const PACKAGES: &str = "Package: claude-code
Version: 2.1.310-1
Architecture: amd64

Package: claude-code
Version: 2.1.325-1
Architecture: amd64

Package: claude-code-helper
Version: 2.1.200-1
Architecture: amd64
";

/// The request a command line of `create-session-image` and `flags` makes.
fn parsed(flags: &[&str]) -> Result<Request, Failure> {
    let words = iter::once(&"create-session-image").chain(flags).map(|word| String::from(*word));
    match args::parse(words)? {
        Command::CreateSessionImage(request) => Ok(request),
        _ => panic!("create-session-image parses as itself"),
    }
}

/// The message a command line of `flags` is refused with.
fn refusal(flags: &[&str]) -> String {
    let Err(failure) = parsed(flags) else {
        panic!("{flags:?} is refused");
    };
    String::from(failure.message())
}

/// A release site whose every channel names `version` and lists [`PACKAGES`], keeping no record of what was asked.
struct Serves(&'static str);

impl Releases for Serves {
    fn served(&self, _channel: Channel) -> Result<Served, Failure> {
        Ok(Served { version: String::from(self.0), packages: String::from(PACKAGES) })
    }
}

/// What `claude` resolves to against a site whose channels name `version`.
fn resolved(claude: Claude, version: &'static str) -> Result<Resolved, String> {
    claude.resolve(&Serves(version)).map_err(|failure| String::from(failure.message()))
}

#[test]
fn an_image_is_never_made_without_a_claude_named() {
    // Catches `--claude` given a default, so an image holds whichever Claude Code was current when it was made.
    assert!(refusal(&[]).contains("--claude"));
    assert!(refusal(&["--uid", "1001"]).contains("--claude"));
}

#[test]
fn claude_takes_a_version_or_latest_or_stable_and_nothing_else() {
    // Catches a channel word misspelled or invented passing as a version, and a version written loosely (a `v`, a
    // sign, a missing part) reaching the repository's package name.
    let Ok(latest) = parsed(&["--claude", "latest"]) else {
        panic!("latest is a channel");
    };
    let Ok(stable) = parsed(&["--claude", "stable"]) else {
        panic!("stable is a channel");
    };
    assert_eq!(latest.claude, Claude::Channel(Channel::Latest));
    assert_eq!(stable.claude, Claude::Channel(Channel::Stable));

    for word in ["nightly", "Latest", "v2.1.310", "2.1", "2.1.310.1", "2.1.+310", "2..310", ""] {
        assert!(refusal(&["--claude", word]).contains("--claude takes a version"), "{word:?} is refused");
    }
}

#[test]
fn the_rest_of_the_command_line_is_refused_where_it_does_not_read() {
    // Catches a uid that is not a number, a flag with no value, an unknown flag (`--add` among them: nothing is
    // copied in from the machine), and a Rust version whose base image has no pinned digest, any of which would build
    // an image other than the one asked for.
    assert!(refusal(&["--claude", "2.1.310", "--uid", "session"]).contains("id=session"));
    assert!(refusal(&["--claude", "2.1.310", "--gid"]).starts_with("usage"));
    assert!(refusal(&["--claude", "2.1.310", "--add", "x=/y"]).starts_with("usage"));
    assert!(refusal(&["--claude", "2.1.310", "--rust", "1.80.0"]).contains("rust=1.80.0"));
}

#[test]
fn a_version_named_is_installed_from_the_latest_repository_and_printed_alone() {
    // Catches a version named looked up in, or installed from, the stable repository, which holds a fraction of the
    // history, and a channel printed for a version the owner named.
    let Ok(request) = parsed(&["--claude", "2.1.310"]) else {
        panic!("a version reads");
    };
    let Ok(resolved) = resolved(request.claude, "2.1.311") else {
        panic!("the repository lists 2.1.310");
    };
    let build = request.build(resolved);

    assert_eq!(request.claude.printed(resolved), "claude=2.1.310");
    assert_eq!(build.tag, "aethereal-session:cc-2.1.310");
    assert!(build.args.contains(&("CLAUDE_CHANNEL", String::from("latest"))));
}

#[test]
fn a_channel_resolves_to_the_version_its_release_names_and_the_image_carries_it() {
    // Catches a channel's name reaching the package or the tag in place of its version, so an image tagged
    // `cc-latest` holds whatever was latest the day it was made, and the channel's own repository not the one it
    // installs from.
    let Ok(request) = parsed(&["--claude", "stable"]) else {
        panic!("stable is a channel");
    };
    let Ok(resolved) = resolved(request.claude, "2.1.325\n") else {
        panic!("the release names a listed version");
    };
    let build = request.build(resolved);

    assert_eq!(request.claude.printed(resolved), "claude=2.1.325 channel=stable");
    assert_eq!(build.tag, "aethereal-session:cc-2.1.325");
    assert!(build.args.contains(&("CLAUDE_VERSION", String::from("2.1.325"))));
    assert!(build.args.contains(&("CLAUDE_CHANNEL", String::from("stable"))));
}

#[test]
fn a_version_the_repository_does_not_list_is_refused_before_a_build() {
    // Catches a version the repository never had (one before its history, or one that does not exist) reaching a
    // build that fails late, a version found only in another package's stanza taken as claude-code's, and whatever
    // the release site serves, an error page, taken as a version.
    let unlisted = resolved(Claude::Version(version("2.1.88")), "unread");
    let other_package = resolved(Claude::Version(version("2.1.200")), "unread");
    let page = resolved(Claude::Channel(Channel::Stable), "<html>not found</html>");
    let unlisted_channel = resolved(Claude::Channel(Channel::Latest), "2.1.326");

    assert!(unlisted.is_err_and(|refused| refused.contains("latest apt repository lists no claude-code 2.1.88-1")));
    assert!(other_package.is_err_and(|refused| refused.contains("claude-code 2.1.200-1")));
    assert!(page.is_err_and(|refused| refused.contains("channel=stable")));
    assert!(unlisted_channel.is_err_and(|refused| refused.contains("claude-code 2.1.326-1")));
}

/// The version `text` writes, which the test writes as one.
fn version(text: &str) -> super::claude::Version {
    let Some(version) = super::claude::Version::read(text) else {
        panic!("{text} is a version");
    };
    version
}

#[test]
fn the_build_hands_the_dockerfile_every_argument_it_declares_with_what_is_pinned() {
    // Catches the pinned fingerprint or digest not reaching the build, a build argument renamed on one side only, so
    // the Dockerfile builds with it empty, and `--uid`, `--gid`, or `--tag` dropped on the way.
    let flags = ["--claude", "2.1.310", "--uid", "1001", "--gid", "1002", "--tag", "session:mine"];
    let Ok(request) = parsed(&flags) else {
        panic!("the command line reads");
    };
    let Ok(resolved) = resolved(request.claude, "unread") else {
        panic!("the repository lists 2.1.310");
    };
    let build = request.build(resolved);
    let dockerfile = include_str!("../../../crates/aethereal-sessions/image/Dockerfile");
    let Ok(base) = Base::toolchain() else {
        panic!("the toolchain's base is pinned");
    };

    for (name, _) in &build.args {
        let declared = dockerfile.lines().any(|line| line == format!("ARG {name}"));
        assert!(declared, "the Dockerfile declares {name}, with no default of its own");
    }
    let expected = [
        ("BASE_IMAGE", format!("rust:{}-slim@{}", base.rust, base.digest)),
        ("CLAUDE_CHANNEL", String::from("latest")),
        ("CLAUDE_VERSION", String::from("2.1.310")),
        ("CLAUDE_RELEASE_FINGERPRINT", String::from(pins::CLAUDE_RELEASE_FINGERPRINT)),
        ("SESSION_UID", String::from("1001")),
        ("SESSION_GID", String::from("1002")),
    ];
    assert_eq!(build.args, expected);
    assert_eq!(build.tag, "session:mine");
}

#[test]
fn the_repository_s_toolchain_has_a_pinned_base_image() {
    // Catches `rust-toolchain.toml` moved on with no digest pinned for its Rust image, so the default build is
    // refused, or the image built on another toolchain, so a session's cargo has rustup fetch the pinned one as a user
    // who cannot write rustup's home.
    let base = Base::toolchain();
    assert!(base.is_ok(), "{:?}", base.err().map(|failure| String::from(failure.message())));
}

#[test]
fn the_context_holds_the_image_s_directory_and_goes_with_the_build() {
    // Catches the Dockerfile, the prompt, the committed release key, or a file the image copies in, the start script,
    // the credential helper, and git's system config, left out of the context, and a context left behind under the
    // target directory after a build.
    let root = env::temp_dir().join(format!("aethereal-xtask-session-image-{}", process::id()));
    let Ok(scratch) = Scratch::write(root.clone()) else {
        panic!("the context is written");
    };
    let context = scratch.context();
    let files = [
        "Dockerfile",
        "prompt.txt",
        "claude-code.asc",
        "aethereal-session-start",
        "git-credential-aethereal",
        "gitconfig",
    ];
    for file in files {
        assert!(context.join(file).is_file(), "the context holds {file}");
    }

    drop(scratch);
    assert!(!root.exists());
}
