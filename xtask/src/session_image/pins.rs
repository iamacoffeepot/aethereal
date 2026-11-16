//! What the session image's build trusts, pinned in one place (ADR-0057) and handed to its Dockerfile as build
//! arguments. Everything else in the image is fetched in the build and verified against these: Claude Code's apt
//! repository against the release key, `claude-code.asc` beside the Dockerfile, whose fingerprint the build checks
//! against this one; and the Debian packages and the toolchain's components against the keys and checksums inside
//! the base image its digest fixes. A part added to the image is added the same way, fetched and verified, with
//! whatever it is verified by pinned here; never copied from the machine that builds it.

/// The fingerprint of Anthropic's Claude Code release signing key, as Anthropic publishes it in Claude Code's setup
/// documentation. The build refuses the committed `claude-code.asc` unless it is this one key, and then apt trusts
/// only it with Claude Code's repository.
pub const CLAUDE_RELEASE_FINGERPRINT: &str = "31DDDE24DDFAB679F42D7BD2BAA929FF1A7ECACE";

/// Each Rust version the image can be built on, and the digest of its `rust:<version>-slim` image: the multi-platform
/// index Docker Hub served for that tag when it was pinned, read with `docker buildx imagetools inspect`. The build
/// pulls the base by digest, so a tag moved since is never what it builds on.
pub const RUST_IMAGES: [(&str, &str); 1] =
    [("1.97.1", "sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c")];
