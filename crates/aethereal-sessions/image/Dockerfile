# check=skip=InvalidDefaultArgInFrom
# The image every Claude session of the hub runs in (ADR-0057). The check skipped above is the empty default of
# `BASE_IMAGE`, which has none on purpose: the xtask always names it.
#
# Made only by `cargo xtask create-session-image --claude <version | latest | stable>`, which writes this directory as
# the build's context and hands the build every argument below: what is pinned (the release key's fingerprint and the
# base image's digest) is in the xtask, in one place, never here, so this file holds no default for them. Only Docker
# is assumed on the machine: every part of the image is fetched in the build and verified against a key, a digest, or
# a checksum that is pinned, never copied from the machine.
#
# The hub never builds an image: its `sessions.image` setting names the tag each session's container runs. Its
# `sessions.user` setting, `1000:1000` by default, must be the uid and gid built in here, the xtask's `--uid` and
# `--gid`.
#
# Each session's container mounts named volumes the hub makes at the directories made below: its home at
# `/session/home`, its cargo target directory at `/session/target`, and the cargo home every session shares at
# `/session/cargo`. Docker fills a new, empty volume from the image's directory at its target, that directory's owner
# with it, so each volume is the session user's from its first container, and no step run as root is needed.

# The Rust image of the repository's `rust-toolchain.toml`, named by its digest: `rust:<version>-slim@sha256:…`. Its
# digest fixes the Debian inside it, whose archive keys check every Debian package apt installs below.
ARG BASE_IMAGE

# What a release channel serves, read before the build: the xtask writes this stage out with `docker build --target
# release --output`, for the channel's version and its apt repository's index of the build's architecture, and it is
# never part of the image.
FROM scratch AS release
ARG CLAUDE_CHANNEL
ARG TARGETARCH
ADD https://downloads.claude.ai/claude-code-releases/${CLAUDE_CHANNEL} /version
ADD https://downloads.claude.ai/claude-code/apt/${CLAUDE_CHANNEL}/dists/${CLAUDE_CHANNEL}/main/binary-${TARGETARCH}/Packages /Packages

# Anthropic's Claude Code release key, `claude-code.asc` beside this file, refused unless it is one key whose
# fingerprint is the pinned one. gpg reads it here, so gpg is not in the image.
FROM ${BASE_IMAGE} AS key
ARG CLAUDE_RELEASE_FINGERPRINT
RUN apt-get update \
    && apt-get install --yes --no-install-recommends gnupg \
    && rm -rf /var/lib/apt/lists/*
COPY claude-code.asc /key/claude-code.asc
RUN set -eu; \
    export GNUPGHOME="$(mktemp --directory)"; \
    keys=$(gpg --batch --with-colons --show-keys /key/claude-code.asc); \
    primaries=$(echo "${keys}" | awk -F: '$1 == "pub" { n++ } END { print n + 0 }'); \
    fingerprint=$(echo "${keys}" | awk -F: '$1 == "fpr" { print $10; exit }'); \
    if [ "${primaries}" != 1 ] || [ "${fingerprint}" != "${CLAUDE_RELEASE_FINGERPRINT}" ]; then \
        echo "refused: claude-code.asc is ${primaries} key(s), fingerprint ${fingerprint}, not the pinned ${CLAUDE_RELEASE_FINGERPRINT}" >&2; \
        exit 1; \
    fi

FROM ${BASE_IMAGE}

ARG CLAUDE_CHANNEL
ARG CLAUDE_VERSION
ARG SESSION_UID
ARG SESSION_GID

# The toolchain's components and target, so a session never has rustup fetch another; rustup checks each against its
# channel's manifest.
RUN rustup component add clippy rustfmt rust-src \
    && rustup target add wasm32-unknown-unknown

# git: a session's work tree is a clone of the hub's repository, which it commits in and pushes its own branches from.
# ca-certificates: cargo and Claude Code reach crates.io, GitHub, and Anthropic over TLS, as apt does Anthropic's
# repository below.
# ripgrep: the fast search Claude Code reaches for in a large tree.
# jq: reading and writing JSON from the shell, as Claude Code's own stream and Docker's answers are.
# python3: small scripts, which a coding session writes often.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates git jq python3 ripgrep \
    && rm -rf /var/lib/apt/lists/*

# Claude Code, at the exact version asked, from Anthropic's apt repository of the channel: apt verifies the
# repository's index against the release key alone, and the package against the index's checksum.
COPY --from=key /key/claude-code.asc /etc/apt/keyrings/claude-code.asc
RUN echo "deb [signed-by=/etc/apt/keyrings/claude-code.asc] https://downloads.claude.ai/claude-code/apt/${CLAUDE_CHANNEL} ${CLAUDE_CHANNEL} main" \
        > /etc/apt/sources.list.d/claude-code.list \
    && apt-get update \
    && apt-get install --yes --no-install-recommends "claude-code=${CLAUDE_VERSION}-1" \
    && rm -rf /var/lib/apt/lists/*

# The bootstrap container's command, which readies the session's work tree from the hub's repository and exits before
# Claude Code's container is made; the credential helper that answers git with the session's bearer, read from the
# container's environment; and the system config that names it (ADR-0057). Each is beside this file.
COPY --chmod=0755 aethereal-session-start git-credential-aethereal /usr/local/bin/
COPY --chmod=0644 gitconfig /etc/gitconfig

# Claude Code refuses to skip its permission checks as root, so the session runs as a user of its own.
RUN groupadd --gid "${SESSION_GID}" session \
    && useradd --uid "${SESSION_UID}" --gid "${SESSION_GID}" --home-dir /session/home --no-create-home \
        --shell /bin/bash session \
    && mkdir --parents /session/home /session/target /session/cargo \
    && chown "${SESSION_UID}:${SESSION_GID}" /session/home /session/target /session/cargo

USER ${SESSION_UID}:${SESSION_GID}
WORKDIR /session/home
