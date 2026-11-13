//! The sessions capability (ADR-0057): Claude sessions the hub runs in containers of their own.
//!
//! Its root here is [`Docker`], the hub's client of Docker's Engine API over its Unix socket: it makes, starts, stops,
//! inspects, lists, and removes containers from typed requests, attaches to a container's stdin and output, reads its
//! logs, and follows the events of the containers that carry a label, each as a typed stream. No request reaches
//! Docker that its asker wrote: a container is made only from a [`ContainerSpec`].

mod docker;

pub use docker::{
    API_VERSION, Access, Acted, Attach, Attached, Attaching, Bind, Calling, ContainerId, ContainerSpec, ContainerState,
    Create, Created, Detached, Dialing, Docker, DockerConfig, DockerError, DockerSettings, Event, Events, EventsDone,
    Fetches, Inspect, Inspected, Kill, Label, List, Listed, Listing, Logging, Logs, LogsDone, Network, Output, Phase,
    Remove, SocketUnaddressable, Start, Std, Stdin, Stop, Until, Unwritable, Variable, Watching,
};
