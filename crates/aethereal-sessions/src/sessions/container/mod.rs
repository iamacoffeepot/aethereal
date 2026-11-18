//! A session's container, made only from the capability's settings and the session's own facts (ADR-0057): its
//! number, its run, the conversation Claude named, the credential, the bearer of its route, and its drop box; and the
//! named volumes it mounts, built from the settings and the number. Nothing a caller asked reaches either.

#[cfg(test)]
mod tests;

use super::Bearer;
use super::record::Conversation;
use super::settings::{CARGO, CLAUDE, HOME, SESSION_LABEL, SessionsSettings, TARGET, Unlabelled};
use crate::{Access, Bind, ContainerSpec, CreateVolume, Label, Network, Stdin, Variable, Volume, VolumeName};
use aethereal_data::write_json_string;
use aethereal_store::DROP_BOX;

/// The variable that holds the bearer of a session's route in its container's environment, which its MCP config
/// names, so the bearer is never an argument of its command.
pub const BEARER_VARIABLE: &str = "AETHEREAL_HUB_BEARER";

/// What a session's container is made for: the session, its run, the conversation it carries on, the credential's
/// variables, the bearer of its route to the hub's tools, and the absolute path of its drop box.
pub struct Made<'a> {
    pub session: u32,
    pub run: u32,
    pub conversation: &'a Conversation,
    pub env: Vec<Variable>,
    pub bearer: Bearer,
    pub drop_box: String,
}

/// Why a session's container, or a volume it mounts, could not be made from the settings.
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

/// Whose a volume a session's container mounts is.
#[derive(Clone, Copy)]
enum Kept {
    /// The session's alone, labelled with its number.
    Own,
    /// Every session's of the hub, labelled with the hub's labels alone.
    Shared,
}

/// A named volume a session's container mounts: its name, where it is mounted, and whose it is.
struct Shelf {
    name: VolumeName,
    target: &'static str,
    kept: Kept,
}

/// The named volumes the session numbered `session` mounts, each named for the hub's label, so two hubs on one daemon
/// never share one: its home, `aethereal-session-<label>-<n>`; its cargo target directory,
/// `aethereal-session-<label>-<n>-target`; and the cargo home every session of the hub shares,
/// `aethereal-cargo-<label>`.
fn shelves(settings: &SessionsSettings, session: u32) -> Result<[Shelf; 3], Unmade> {
    let label = &settings.label;
    let named = |name: String| VolumeName::read(&name).ok_or(Unmade::Unnamed);

    Ok([
        Shelf { name: named(format!("aethereal-session-{label}-{session}"))?, target: HOME, kept: Kept::Own },
        Shelf { name: named(format!("aethereal-session-{label}-{session}-target"))?, target: TARGET, kept: Kept::Own },
        Shelf { name: named(format!("aethereal-cargo-{label}"))?, target: CARGO, kept: Kept::Shared },
    ])
}

/// The requests that make the volumes the session numbered `session` mounts, labelled, in the order they are made.
///
/// # Errors
///
/// As [`Unmade`] says.
pub fn volumes(settings: &SessionsSettings, session: u32) -> Result<Vec<CreateVolume>, Unmade> {
    let own = labels(settings, session)?;
    let shared = settings.container_labels()?;
    let made = shelves(settings, session)?.map(|shelf| {
        let labels = match shelf.kept {
            Kept::Own => own.clone(),
            Kept::Shared => shared.clone(),
        };
        CreateVolume { name: shelf.name, labels }
    });

    Ok(Vec::from(made))
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

/// The spec of the container `made` describes, as `settings` say every session's is made.
///
/// # Errors
///
/// As [`Unmade`] says.
pub fn spec(settings: &SessionsSettings, made: Made<'_>) -> Result<ContainerSpec, Unmade> {
    let Made { session, run, conversation, env, bearer, drop_box } = made;
    let places = [("HOME", HOME), ("CARGO_HOME", CARGO), ("CARGO_TARGET_DIR", TARGET)];
    let mut variables: Vec<Variable> =
        places.map(|(name, value)| Variable { name: String::from(name), value: String::from(value) }).to_vec();
    variables.extend(env);
    variables.push(Variable { name: String::from(BEARER_VARIABLE), value: bearer.hex() });
    let volumes =
        shelves(settings, session)?.map(|shelf| Volume { name: shelf.name, target: String::from(shelf.target) });

    Ok(ContainerSpec {
        name: format!("aethereal-session-{}-{session}-{run}", settings.label),
        image: settings.image.clone(),
        user: settings.user.clone(),
        working_dir: String::from(HOME),
        command: command(settings, conversation),
        env: variables,
        // Claude Code is in the image, so nothing of the machine is bound in but the session's drop box, which the
        // store reads only what the session puts in, never following a link.
        binds: vec![Bind { source: drop_box, target: String::from(DROP_BOX), access: Access::ReadWrite }],
        volumes: Vec::from(volumes),
        // A network of its own: the container reaches the hub's tools only by its route, never the owner's port on the
        // loopback.
        network: Network::Bridge,
        millicpus: settings.millicpus,
        memory_bytes: settings.memory_bytes,
        labels: labels(settings, session)?,
        stdin: Stdin::Kept,
    })
}

/// Claude Code in print mode, reading and writing stream-json, on the settings' model, its permissions skipped as the
/// container is the sandbox, told where it is by the settings' prompt, resuming the conversation Claude named, and
/// reaching the hub's tools when the settings name them.
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
        "--append-system-prompt",
        &settings.prompt,
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

/// The MCP config that names the hub's tools at `url`, as Claude Code reads one inline: each call bears the session's
/// bearer, which Claude Code reads from the container's environment as it expands `${…}` in a header, so the config
/// names the variable and never the bearer.
fn mcp_config(url: &str) -> String {
    let mut config = String::from(r#"{"mcpServers":{"aethereal":{"type":"http","url":"#);
    write_json_string(url, &mut config);
    config.push_str(r#","headers":{"Authorization":"#);
    write_json_string(&format!("Bearer ${{{BEARER_VARIABLE}}}"), &mut config);
    config.push_str("}}}}");

    config
}
