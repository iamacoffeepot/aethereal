# The image every Claude session of the hub runs in (ADR-0057).
#
# Build it on the machine the hub runs on, from the repository's root:
#
#     docker build -t aethereal-session crates/aethereal-sessions/image
#
# The hub never builds an image: its `sessions.image` setting names the tag each session's container runs,
# `aethereal-session` by default. Its `sessions.user` setting, `1000:1000` by default, must be the uid and gid built
# in here; build with `--build-arg SESSION_UID=… --build-arg SESSION_GID=…` for another.
#
# The image holds no Claude Code and no credential. The hub binds the build host's `claude` into each container
# read-only, and gives the credential as the container's environment.
#
# Each session's container mounts named volumes the hub makes at the directories made below: its home at
# `/session/home`, its cargo target directory at `/session/target`, and the cargo home every session shares at
# `/session/cargo`. Docker fills a new, empty volume from the image's directory at its target, that directory's owner
# with it, so each volume is the session user's from its first container, and no step run as root is needed.

# The toolchain of the repository's `rust-toolchain.toml`, with its components and target, so a session never has
# rustup fetch another.
ARG RUST_VERSION=1.97.1
FROM rust:${RUST_VERSION}-slim

ARG SESSION_UID=1000
ARG SESSION_GID=1000

RUN rustup component add clippy rustfmt rust-src \
    && rustup target add wasm32-unknown-unknown

# git: Claude Code reads a repository's state, and a session commits its work locally.
# ca-certificates: cargo and Claude Code reach crates.io, GitHub, and Anthropic over TLS.
# ripgrep: the fast search Claude Code reaches for in a large tree.
# jq: reading and writing JSON from the shell, as Claude Code's own stream and Docker's answers are.
# python3: small scripts, which a coding session writes often.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates git jq python3 ripgrep \
    && rm -rf /var/lib/apt/lists/*

# Claude Code refuses to skip its permission checks as root, so the session runs as a user of its own.
RUN groupadd --gid "${SESSION_GID}" session \
    && useradd --uid "${SESSION_UID}" --gid "${SESSION_GID}" --home-dir /session/home --no-create-home \
        --shell /bin/bash session \
    && mkdir --parents /session/home /session/target /session/cargo \
    && chown "${SESSION_UID}:${SESSION_GID}" /session/home /session/target /session/cargo

USER ${SESSION_UID}:${SESSION_GID}
WORKDIR /session/home
