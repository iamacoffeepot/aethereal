//! Docker's replies read as typed answers (ADR-0057): a success as what it carries, and an error status as a named
//! case, with the message Docker gave.

#[cfg(test)]
mod tests;

use super::json::{boolean, elements, field, integer, members, text, tree};
use super::{Acted, ContainerId, ContainerState, Created, DockerError, Inspected, Label, Listed, Listing, Phase};
use aethereal_data::{JsonTree, JsonValue};
use core::str;

/// What an error status Docker answered with says: not found, a conflict, or another, each with Docker's message.
pub(super) fn error(status: u16, body: &[u8]) -> DockerError {
    let message = message(body);
    match status {
        404 => DockerError::NotFound { message },
        409 => DockerError::Conflict { message },
        _ => DockerError::Refused { status, message },
    }
}

/// The message of an error Docker answered with: its body's `message`, or, when the body is not Docker's JSON, its
/// text as it came.
fn message(body: &[u8]) -> String {
    let written = String::from_utf8_lossy(body);
    let read = tree(&written);
    let said = read.as_ref().and_then(|read| field(read, read.root(), "message")).and_then(text);

    said.map_or_else(|| written.trim().to_owned(), str::to_owned)
}

/// What a create came to: `201` with the new container's id.
pub(super) fn created(status: u16, body: &[u8]) -> Created {
    if status != 201 {
        return Created::Failed(error(status, body));
    }
    let id = read(body, |tree| field(tree, tree.root(), "Id").and_then(text).and_then(ContainerId::read));

    id.map_or(Created::Failed(DockerError::Malformed), Created::Made)
}

/// What a start, a stop, a kill, or a remove came to: `204` done, and `304` when there was nothing to do.
pub(super) fn acted(status: u16, body: &[u8]) -> Acted {
    match status {
        204 => Acted::Done,
        304 => Acted::Unchanged,
        _ => Acted::Failed(error(status, body)),
    }
}

/// What an inspect came to: `200` with the container's `State`.
pub(super) fn inspected(status: u16, body: &[u8]) -> Inspected {
    if status != 200 {
        return Inspected::Failed(error(status, body));
    }
    let state = read(body, |tree| field(tree, tree.root(), "State").and_then(|state| container_state(tree, state)));

    state.map_or(Inspected::Failed(DockerError::Malformed), Inspected::State)
}

/// What a list came to: `200` with an array of the containers found.
pub(super) fn listed(status: u16, body: &[u8]) -> Listed {
    if status != 200 {
        return Listed::Failed(error(status, body));
    }
    let listings = read(body, |tree| {
        let found = elements(tree, tree.root())?;

        found.iter().map(|container| listing(tree, container)).collect::<Option<Vec<_>>>()
    });

    listings.map_or(Listed::Failed(DockerError::Malformed), Listed::Containers)
}

/// What `reading` reads from `body` as JSON: none when it is not JSON, or `reading` finds none.
fn read<T>(body: &[u8], reading: impl FnOnce(&JsonTree<'_>) -> Option<T>) -> Option<T> {
    let written = str::from_utf8(body).ok()?;
    let read = tree(written)?;

    reading(&read)
}

/// A container's `State`, as an inspect writes it.
fn container_state(tree: &JsonTree<'_>, state: &JsonValue<'_>) -> Option<ContainerState> {
    let status = field(tree, state, "Status").and_then(text)?;
    let code = field(tree, state, "ExitCode").and_then(integer)?;
    let oom_killed = field(tree, state, "OOMKilled").and_then(boolean)?;
    let read = match phase(status)? {
        Phase::Created => ContainerState::Created,
        Phase::Running => ContainerState::Running,
        Phase::Paused => ContainerState::Paused,
        Phase::Restarting => ContainerState::Restarting,
        Phase::Removing => ContainerState::Removing,
        Phase::Exited => ContainerState::Exited { code, oom_killed },
        Phase::Dead => ContainerState::Dead { code, oom_killed },
    };

    Some(read)
}

/// One container of a list.
fn listing(tree: &JsonTree<'_>, container: &JsonValue<'_>) -> Option<Listing> {
    let id = field(tree, container, "Id").and_then(text).and_then(ContainerId::read)?;
    let names = field(tree, container, "Names").and_then(|names| elements(tree, names))?;
    let name = names.first().and_then(text)?;
    let phase = field(tree, container, "State").and_then(text).and_then(phase)?;
    let labels = field(tree, container, "Labels").and_then(|labels| members(tree, labels))?;
    let labels = labels
        .iter()
        .map(|member| text(&member.value).map(|value| Label { key: member.name.clone(), value: String::from(value) }))
        .collect::<Option<Vec<_>>>()?;

    Some(Listing { id, name: String::from(name.trim_start_matches('/')), phase, labels })
}

/// The phase Docker's name for a container's state names: none for a name it has not used.
fn phase(status: &str) -> Option<Phase> {
    let phase = match status {
        "created" => Phase::Created,
        "running" => Phase::Running,
        "paused" => Phase::Paused,
        "restarting" => Phase::Restarting,
        "removing" => Phase::Removing,
        "exited" => Phase::Exited,
        "dead" => Phase::Dead,
        _ => return None,
    };

    Some(phase)
}
