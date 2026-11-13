//! What a container is made from (ADR-0057): a typed spec, the only thing [`Create`](super::Create) takes, and the
//! JSON body it is written as.

#[cfg(test)]
mod tests;

use super::{Label, Unwritable};
use aethereal_data::{Plain, write_json_string};
use core::fmt::Write as _;

/// What a container is made from: its name, image, user, working directory, command, environment, binds, network,
/// limits, labels, and stdin, each stated by its maker.
///
/// It never runs with a terminal: its stdout and stderr come apart, each chunk behind an 8-byte header naming its
/// stream, which is what an attach and the logs demultiplex.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.container_spec")]
pub struct ContainerSpec {
    /// The container's name, unique on the daemon.
    pub name: String,
    /// The image it runs, by name and tag or by digest.
    pub image: String,
    /// The user its process runs as: a name, or `uid:gid`.
    pub user: String,
    /// The absolute directory its process starts in.
    pub working_dir: String,
    /// The program and its arguments, in place of the image's own command.
    pub command: Vec<String>,
    /// Its environment, in order.
    pub env: Vec<Variable>,
    /// Directories of this machine it sees, each at a path of its own.
    pub binds: Vec<Bind>,
    pub network: Network,
    /// The most CPU time it takes, in thousandths of one CPU: 4000 caps it at four CPUs' worth.
    pub millicpus: u32,
    /// The most memory it holds, past which the kernel kills its process.
    pub memory_bytes: u64,
    /// The labels it carries, which a list and the events are filtered by.
    pub labels: Vec<Label>,
    pub stdin: Stdin,
}

/// One environment variable: a name, which holds no `=`, and its value.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.variable")]
pub struct Variable {
    pub name: String,
    pub value: String,
}

/// A directory of this machine a container sees: `source`, which must already exist, at `target` inside it.
///
/// It is written as one of `HostConfig.Mounts`, a bind with its source and target apart, not as a `source:target`
/// line of `HostConfig.Binds`: no path is split at a colon, and a source that is missing is refused, never made by the
/// daemon as an empty directory its root owns.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.bind")]
pub struct Bind {
    pub source: String,
    pub target: String,
    pub access: Access,
}

/// Whether a container may write to what it is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.access")]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// The network a container joins, written as its `HostConfig.NetworkMode`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.network")]
pub enum Network {
    /// None: it reaches nothing outside itself.
    Isolated,
    /// The daemon's default bridge.
    Bridge,
    /// This machine's own network.
    Host,
    /// A network made on the daemon, by its name.
    Named(String),
}

/// Whether a container's stdin is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.docker.stdin")]
pub enum Stdin {
    /// Open, and kept open when an attach hangs up, so the next attach writes to the same process.
    Kept,
    /// Closed: the process reads nothing.
    Closed,
}

impl Network {
    /// What `HostConfig.NetworkMode` says for it.
    fn mode(&self) -> &str {
        match self {
            Self::Isolated => "none",
            Self::Bridge => "bridge",
            Self::Host => "host",
            Self::Named(name) => name,
        }
    }
}

impl ContainerSpec {
    /// The body of `POST /containers/create` for this spec.
    ///
    /// # Errors
    ///
    /// The variable whose name is empty or holds `=`, which Docker would split elsewhere, and the label whose key is
    /// empty.
    pub(super) fn body(&self) -> Result<String, Unwritable> {
        let open = self.stdin == Stdin::Kept;
        let mut body = String::from("{\"Image\":");
        write_json_string(&self.image, &mut body);
        body.push_str(",\"User\":");
        write_json_string(&self.user, &mut body);
        body.push_str(",\"WorkingDir\":");
        write_json_string(&self.working_dir, &mut body);
        body.push_str(",\"Cmd\":");
        strings(self.command.iter().map(String::as_str), &mut body);
        body.push_str(",\"Env\":");
        let env = self.env.iter().map(Variable::line).collect::<Result<Vec<_>, _>>()?;
        strings(env.iter().map(String::as_str), &mut body);
        body.push_str(",\"Labels\":");
        labels(&self.labels, &mut body)?;
        let _ = write!(
            body,
            ",\"OpenStdin\":{open},\"StdinOnce\":false,\"AttachStdin\":{open},\"AttachStdout\":true,\
             \"AttachStderr\":true,\"Tty\":false"
        );

        body.push_str(",\"HostConfig\":{\"NetworkMode\":");
        write_json_string(self.network.mode(), &mut body);
        let nano_cpus = u64::from(self.millicpus) * 1_000_000;
        let _ = write!(body, ",\"NanoCpus\":{nano_cpus},\"Memory\":{},\"Mounts\":[", self.memory_bytes);
        for (place, bind) in self.binds.iter().enumerate() {
            if place > 0 {
                body.push(',');
            }
            bind.write(&mut body);
        }
        body.push_str("]}}");

        Ok(body)
    }
}

impl Variable {
    /// The variable as Docker's `Env` lists it, `name=value`.
    fn line(&self) -> Result<String, Unwritable> {
        let is_named = !self.name.is_empty() && !self.name.contains('=');
        if !is_named {
            return Err(Unwritable::Variable { name: self.name.clone() });
        }

        Ok(format!("{}={}", self.name, self.value))
    }
}

impl Bind {
    /// Appends the bind as one of `HostConfig.Mounts`.
    fn write(&self, out: &mut String) {
        let read_only = self.access == Access::ReadOnly;
        out.push_str("{\"Type\":\"bind\",\"Source\":");
        write_json_string(&self.source, out);
        out.push_str(",\"Target\":");
        write_json_string(&self.target, out);
        let _ = write!(out, ",\"ReadOnly\":{read_only}}}");
    }
}

/// Appends `items` as a JSON array of strings.
pub(super) fn strings<'s>(items: impl Iterator<Item = &'s str>, out: &mut String) {
    out.push('[');
    for (place, item) in items.enumerate() {
        if place > 0 {
            out.push(',');
        }
        write_json_string(item, out);
    }
    out.push(']');
}

/// Appends `labels` as a JSON object, each key a member.
///
/// # Errors
///
/// The label whose key is empty.
fn labels(labels: &[Label], out: &mut String) -> Result<(), Unwritable> {
    out.push('{');
    for (place, label) in labels.iter().enumerate() {
        if label.key.is_empty() {
            return Err(Unwritable::Label { key: label.key.clone() });
        }
        if place > 0 {
            out.push(',');
        }
        write_json_string(&label.key, out);
        out.push(':');
        write_json_string(&label.value, out);
    }
    out.push('}');

    Ok(())
}
