//! The sessions capability's settings (ADR-0028, ADR-0057): everything a session's container is made from, its
//! limits, and how an idle session is wound down.

use crate::Label;
use aethereal_data::Plain;
use core::error::Error;
use core::fmt;
use core::iter;

/// The label key every session's container carries, whose value names the hub that runs it: what a list and the
/// events are filtered by.
pub const HUB_LABEL: &str = "aethereal.sessions";

/// The label key that names a session's container's session, by its number.
pub const SESSION_LABEL: &str = "aethereal.session";

/// Where the session's home is inside its container, its working directory and its `HOME`.
pub const HOME: &str = "/session/home";

/// Where the session's cargo target directory is inside its container, its `CARGO_TARGET_DIR`.
pub const TARGET: &str = "/session/target";

/// Where the cargo home every session of the hub shares is inside each container, its `CARGO_HOME`.
pub const CARGO: &str = "/session/cargo";

/// What each session is told of where it is, appended to Claude Code's own system prompt: the default of
/// [`SessionsSettings::prompt`], kept beside the session image's Dockerfile.
const PROMPT: &str = include_str!("../../image/prompt.txt");

/// Where the Claude Code binary is inside its container: where the session image's package installs it.
pub const CLAUDE: &str = "/usr/bin/claude";

/// The sessions capability's section, `sessions`: what each session's container is made of, and its limits (ADR-0057).
/// Every field of a container comes from here or from the session's number, never from what a caller asked. Where the
/// sessions' records and transcripts are kept is `Files`' setting for its `Save` namespace, and where each home is
/// kept is Docker's.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.sessions.settings")]
#[settings]
pub struct SessionsSettings {
    /// The file of `KEY=VALUE` lines given to each container as its environment: the Claude Code credential. Only its
    /// path is a setting; it is read by the host as each container is made, never through `Files`, and never written
    /// anywhere else.
    #[flag]
    pub credential: String,
    /// The image each container runs: a session image `cargo xtask create-session-image` made on this machine, by
    /// the tag it printed, which holds Claude Code. It holds `/session/home`, `/session/target`, and `/session/cargo`
    /// owned by [`user`](Self::user), which Docker copies into each new volume mounted there; an image without them
    /// leaves each volume root's, and the session cannot write its home.
    #[flag]
    pub image: String,
    /// The model each session runs, stated as print mode otherwise picks its own.
    #[flag]
    pub model: String,
    /// The user each container's process runs as, `uid:gid`: the one the image was built for, which owns the
    /// directories its volumes are mounted at.
    #[flag]
    pub user: String,
    /// The sessions' route to the hub's tools, as a container on Docker's default bridge reaches it: the port and
    /// interface of the program's container stack, whose actor layer this capability is (ADR-0035, ADR-0057). Empty
    /// for none.
    #[flag]
    pub hub_url: String,
    /// The value of the label every container of this hub carries, [`HUB_LABEL`].
    #[flag]
    pub label: String,
    /// More labels every container carries, each `key=value`.
    pub labels: Vec<String>,
    /// The most sessions awake at once, a hibernating or archived session holding no container: a new session, or a
    /// wake, past it is refused, naming the awake ones.
    #[flag]
    pub most_running: u32,
    /// The seconds an awake session with no turn and no background task waits before it is wound down: told, compacted,
    /// and its container stopped, so it hibernates. 55 minutes by default: Claude's prompt cache keeps a session's
    /// context for an hour, so a session idle that long reads its whole context afresh on its next turn whatever the
    /// hub does, and the five minutes left leave the wind-down's own turns, the notice and `/compact`, reading it warm.
    #[flag]
    pub idle_secs: u32,
    /// What the hub tells a session before it compacts it to hibernate, `{minutes}` standing for its idle minutes.
    pub notice: String,
    /// What each session is told of where it is, appended to Claude Code's system prompt.
    pub prompt: String,
    /// The seconds the notice's turn, and then the compaction, are each given before the session's container is
    /// stopped anyway.
    #[flag]
    pub wind_down_secs: u32,
    /// The most CPU time a container takes, in thousandths of one CPU.
    #[flag]
    pub millicpus: u32,
    /// The most memory a container holds.
    #[flag]
    pub memory_bytes: u64,
    /// The seconds a container is given to stop before it is killed.
    #[flag]
    pub grace_secs: u32,
    /// The seconds each request of Docker is given, its whole exchange.
    #[flag]
    pub deadline_secs: u32,
}

impl Default for SessionsSettings {
    fn default() -> Self {
        Self {
            credential: String::from("sessions/claude.env"),
            image: String::from("aethereal-session"),
            model: String::from("opus"),
            user: String::from("1000:1000"),
            hub_url: String::from("http://172.17.0.1:8893/mcp"),
            label: String::from("hub"),
            labels: Vec::new(),
            most_running: 4,
            idle_secs: 55 * 60,
            notice: String::from(
                "The hub: this session has been idle for {minutes} minutes, and it will be compacted and hibernated \
                 next. Before that, write down your loose threads: unfinished work, open questions, uncommitted \
                 changes, and what to pick up next.",
            ),
            prompt: String::from(PROMPT.trim_end()),
            wind_down_secs: 10 * 60,
            millicpus: 4000,
            memory_bytes: 8 << 30,
            grace_secs: 10,
            deadline_secs: 60,
        }
    }
}

impl SessionsSettings {
    /// The labels every container of this hub carries: the hub's, then each of `labels`.
    ///
    /// # Errors
    ///
    /// The label of `labels` that is not `key=value` with a key.
    pub fn container_labels(&self) -> Result<Vec<Label>, Unlabelled> {
        let hub = Label { key: String::from(HUB_LABEL), value: self.label.clone() };
        let more = self.labels.iter().map(|written| {
            let split = written.split_once('=').filter(|(key, _)| !key.is_empty());
            split
                .map(|(key, value)| Label { key: String::from(key), value: String::from(value) })
                .ok_or_else(|| Unlabelled { label: written.clone() })
        });

        iter::once(Ok(hub)).chain(more).collect()
    }

    /// The label a list and the events are filtered by: the hub's.
    #[must_use]
    pub fn hub_label(&self) -> Label {
        Label { key: String::from(HUB_LABEL), value: self.label.clone() }
    }

    /// The notice a session about to hibernate is told, its idle minutes written in.
    #[must_use]
    pub fn notice(&self) -> String {
        self.notice.replace("{minutes}", &(self.idle_secs / 60).to_string())
    }
}

/// Why the sessions capability did not start: a label of its settings is not `key=value` with a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlabelled {
    /// The label as the settings write it.
    pub label: String,
}

impl fmt::Display for Unlabelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`sessions.labels` holds `{}`, which is not `key=value` with a key", self.label)
    }
}

impl Error for Unlabelled {}
