//! A session's container, made only from the capability's settings and the session's own facts (ADR-0057): its
//! number, its run, the conversation Claude named, its home, and the credential. Nothing a caller asked reaches it.

#[cfg(test)]
mod tests;

use super::record::Conversation;
use super::settings::{CLAUDE, HOME, SESSION_LABEL, SessionsSettings, Unlabelled};
use crate::{Access, Bind, ContainerSpec, Label, Network, Stdin, Variable};
use aethereal_data::write_json_string;

/// What a session's container is made for: the session, its run, the conversation it carries on, the absolute path of
/// its home on this machine, and the credential's variables.
pub struct Made<'a> {
    pub session: u32,
    pub run: u32,
    pub conversation: &'a Conversation,
    pub home: String,
    pub env: Vec<Variable>,
}

/// The spec of the container `made` describes, as `settings` say every session's is made.
///
/// # Errors
///
/// A label of the settings that is not `key=value` with a key.
pub fn spec(settings: &SessionsSettings, made: Made<'_>) -> Result<ContainerSpec, Unlabelled> {
    let Made { session, run, conversation, home, env } = made;
    let mut labels = settings.container_labels()?;
    labels.push(Label { key: String::from(SESSION_LABEL), value: session.to_string() });
    let mut variables = vec![Variable { name: String::from("HOME"), value: String::from(HOME) }];
    variables.extend(env);
    let binds = vec![
        Bind { source: home, target: String::from(HOME), access: Access::ReadWrite },
        Bind { source: settings.claude.clone(), target: String::from(CLAUDE), access: Access::ReadOnly },
    ];

    Ok(ContainerSpec {
        name: format!("aethereal-session-{}-{session}-{run}", settings.label),
        image: settings.image.clone(),
        user: settings.user.clone(),
        working_dir: String::from(HOME),
        command: command(settings, conversation),
        env: variables,
        binds,
        network: Network::Host,
        millicpus: settings.millicpus,
        memory_bytes: settings.memory_bytes,
        labels,
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
