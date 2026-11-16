//! A session's container, made only from the capability's settings and the session's own facts (ADR-0057): its
//! number, its run, the conversation Claude named, and the credential; and its home, a named volume built from the
//! settings and the number. Nothing a caller asked reaches either, nor the container that gives the home to the
//! session's user.

#[cfg(test)]
mod tests;

use super::record::Conversation;
use super::settings::{CLAUDE, HOME, SESSION_LABEL, SessionsSettings, Unlabelled};
use crate::{Access, Bind, ContainerSpec, Label, Network, Stdin, Variable, Volume, VolumeName};
use aethereal_data::write_json_string;

/// What a session's container is made for: the session, its run, the conversation it carries on, and the credential's
/// variables.
pub struct Made<'a> {
    pub session: u32,
    pub run: u32,
    pub conversation: &'a Conversation,
    pub env: Vec<Variable>,
}

/// Why a session's container, or the one that gives its home to its user, could not be made from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmade {
    /// A label of the settings is not `key=value` with a key.
    Unlabelled(Unlabelled),
    /// The settings' label makes no name Docker takes for a volume.
    Unnamed,
}

impl From<Unlabelled> for Unmade {
    fn from(unlabelled: Unlabelled) -> Self {
        Self::Unlabelled(unlabelled)
    }
}

/// The named volume that is the home of the session numbered `session`: named for the hub's label and the number, so
/// two hubs on one daemon never share one.
///
/// # Errors
///
/// The settings' label makes no name Docker takes.
pub fn home(settings: &SessionsSettings, session: u32) -> Result<VolumeName, Unmade> {
    VolumeName::read(&format!("aethereal-session-{}-{session}", settings.label)).ok_or(Unmade::Unnamed)
}

/// The labels every container and volume of the session numbered `session` carries: the hub's, the settings', and the
/// session's.
///
/// # Errors
///
/// A label of the settings is not `key=value` with a key.
pub fn labels(settings: &SessionsSettings, session: u32) -> Result<Vec<Label>, Unlabelled> {
    let mut labels = settings.container_labels()?;
    labels.push(Label { key: String::from(SESSION_LABEL), value: session.to_string() });

    Ok(labels)
}

/// The spec of the container that gives the home of the session numbered `session` to the session's user, made for its
/// run `run`, before its first container: the settings' image, run as root as only root may change an owner, with no
/// network, no credential, and nothing of this machine's, changing the owner of the home's top directory alone and
/// ending.
///
/// # Errors
///
/// As [`Unmade`] says.
pub fn owner(settings: &SessionsSettings, session: u32, run: u32) -> Result<ContainerSpec, Unmade> {
    let command = ["chown", &settings.user, HOME].map(String::from).to_vec();

    Ok(ContainerSpec {
        name: format!("aethereal-session-{}-{session}-{run}-home", settings.label),
        image: settings.image.clone(),
        user: String::from("0:0"),
        working_dir: String::from("/"),
        command,
        env: Vec::new(),
        binds: Vec::new(),
        volumes: vec![Volume { name: home(settings, session)?, target: String::from(HOME) }],
        network: Network::Isolated,
        millicpus: settings.millicpus,
        memory_bytes: settings.memory_bytes,
        labels: labels(settings, session)?,
        stdin: Stdin::Closed,
    })
}

/// The spec of the container `made` describes, as `settings` say every session's is made.
///
/// # Errors
///
/// As [`Unmade`] says.
pub fn spec(settings: &SessionsSettings, made: Made<'_>) -> Result<ContainerSpec, Unmade> {
    let Made { session, run, conversation, env } = made;
    let mut variables = vec![Variable { name: String::from("HOME"), value: String::from(HOME) }];
    variables.extend(env);
    let binds = vec![Bind { source: settings.claude.clone(), target: String::from(CLAUDE), access: Access::ReadOnly }];

    Ok(ContainerSpec {
        name: format!("aethereal-session-{}-{session}-{run}", settings.label),
        image: settings.image.clone(),
        user: settings.user.clone(),
        working_dir: String::from(HOME),
        command: command(settings, conversation),
        env: variables,
        binds,
        volumes: vec![Volume { name: home(settings, session)?, target: String::from(HOME) }],
        network: Network::Host,
        millicpus: settings.millicpus,
        memory_bytes: settings.memory_bytes,
        labels: labels(settings, session)?,
        stdin: Stdin::Kept,
    })
}

/// Claude Code in print mode, reading and writing stream-json, on the settings' model, its permissions skipped as the
/// container is the sandbox, resuming the conversation Claude named, and reaching the hub's tools when the settings
/// name them.
fn command(settings: &SessionsSettings, conversation: &Conversation) -> Vec<String> {
    let words = [
        CLAUDE,
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        &settings.model,
        "--dangerously-skip-permissions",
    ];
    let mut command: Vec<String> = words.iter().map(|word| String::from(*word)).collect();
    if let Conversation::Begun(id) = conversation {
        command.extend([String::from("--resume"), id.clone()]);
    }
    let reaches_hub = !settings.hub_url.is_empty();
    if reaches_hub {
        command.extend([String::from("--mcp-config"), mcp_config(&settings.hub_url)]);
    }

    command
}

/// The MCP config that names the hub's tools at `url`, as Claude Code reads one inline.
fn mcp_config(url: &str) -> String {
    let mut config = String::from(r#"{"mcpServers":{"aethereal":{"type":"http","url":"#);
    write_json_string(url, &mut config);
    config.push_str("}}}");

    config
}
