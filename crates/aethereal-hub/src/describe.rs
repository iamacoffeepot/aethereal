//! The hub's own tool `describe_hub`, which answers the commit the hub was built from and the link revision it speaks
//! (ADR-0038), so an agent can tell which tools and fixes the hub it talks to has.

use aethereal_actor::{
    Actor, ActorRef, Anyone, Anywhere, Beginning, ChildOf, Ending, Fault, Handles, ProtocolRef, ToSender, Turn,
};
use aethereal_data::{Plain, Schema, SchemaNodes, to_bytes};
use aethereal_link::REVISION;
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};

/// The tool's name.
const DESCRIBE_HUB: &str = "describe_hub";
/// The commit the build script read: empty when it read none.
const COMMIT: &str = env!("AETHEREAL_BUILD_COMMIT");

/// Asks what the hub was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.hub.describe_hub")]
pub struct DescribeHub;

/// What the hub was built from and the link revision it speaks.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.hub.hub_build")]
pub struct HubBuild {
    pub source: Source,
    /// The revision of the link's frames: an engine that says another in its `Hello` is refused (ADR-0038).
    pub link_revision: u32,
}

/// The source a build was made from.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.hub.source")]
pub enum Source {
    /// The git commit checked out as it was built. Changes not committed are not said.
    Commit { commit: String },
    /// Built where git read no commit, as from a source tarball or with no git installed.
    Unknown,
}

impl Source {
    /// The source this program was built from.
    fn built() -> Self {
        if COMMIT.is_empty() {
            return Self::Unknown;
        }

        Self::Commit { commit: String::from(COMMIT) }
    }
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.hub.describe_hub_tool_config")]
pub struct DescribeHubToolConfig;

/// The tool `describe_hub`: answers what the hub was built from.
#[derive(Debug, Plain)]
#[kind(name = "aether.hub.describe_hub_tool")]
pub struct DescribeHubTool;

impl ChildOf for DescribeHubTool {
    type Parent = Anywhere;
}

impl Actor for DescribeHubTool {
    type Config = DescribeHubToolConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: DescribeHubToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for DescribeHubTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::json::<DescribeHub>(
            DESCRIBE_HUB,
            "Describes this hub's build: `source`, the git commit it was built from, or `Unknown` when it was built \
             where git read none; and `link_revision`, the revision of the link it speaks, which every engine linked \
             to it speaks too, as an engine of another is refused.",
            Output::Value(SchemaNodes::of(&HubBuild::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for DescribeHubTool {
    type Sends = (ToSender<Called>,);
    type From = Anyone;

    /// Its input holds nothing, so it is not read.
    fn handle(&mut self, _: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let build = HubBuild { source: Source::built(), link_revision: REVISION };
        let called = Called::output(turn, to_bytes(&build)?);
        turn.reply(&called)?;

        Ok(())
    }
}
