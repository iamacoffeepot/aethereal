//! What crosses between a holder and a tool's worker (ADR-0037), and the command line a tool's sheet carries
//! (ADR-0053).

use aethereal_actor::{Bytes, Carries, Protocol, ProtocolRef, Turn};
use aethereal_data::config::{Declared, Flagged, Suggested};
use aethereal_data::{Bound, Plain, Schema, SchemaNodes};

/// What a tool's worker speaks: each [`Call`], asked of it and answered with [`Called`].
pub struct Tooling;

impl Protocol for Tooling {
    type Kinds = (Call,);
}

impl Carries<Call> for Tooling {
    type Answers = (Called,);
}

/// A call of `tool`, named as one worker may serve many, with `input` a value of the tool's input schema, as its
/// bytes in a blob (ADR-0042).
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tools.call")]
pub struct Call {
    pub tool: String,
    pub input: Bytes,
}

/// What a call came to.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.tools.called")]
pub enum Called {
    /// A value of the tool's output schema, as its bytes in a blob (ADR-0042).
    Output(Bytes),
    /// A PNG, in a blob, and `text` for the agent to read beside it: what it cannot read off the pixels (ADR-0043).
    Image { png: Bytes, text: String },
    /// The tool could not do it, and says why, for the agent to read.
    Refused(String),
}

impl Called {
    /// The output whose bytes, a value of the tool's output schema, are `bytes`, put in `turn`: refused, for the agent
    /// to read, when the engine has no room to hold it (ADR-0014). A buffer the turn handed out is put without a copy.
    pub fn output<A, S>(turn: &mut Turn<'_, A, S>, bytes: Vec<u8>) -> Self {
        turn.put(bytes)
            .map_or_else(|_| Self::Refused(String::from("the engine had no room to hold the output")), Self::Output)
    }

    /// The image whose PNG is `png`, put in `turn`, with `text` beside it: refused, for the agent to read, when the
    /// engine has no room to hold it (ADR-0014).
    pub fn image<A, S>(turn: &mut Turn<'_, A, S>, png: Vec<u8>, text: String) -> Self {
        turn.put(png).map_or_else(
            |_| Self::Refused(String::from("the engine had no room to hold the image")),
            |png| Self::Image { png, text },
        )
    }
}

/// What a tool answers that has nothing to tell but that it was done.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.done")]
pub struct Done {}

/// One tool as a worker offers it: its name, what it does, its input's schema and the flags its command line is typed
/// with (ADR-0053), and what it answers with.
///
/// A tool with no flags takes its input as one JSON word, or none for an input of no fields. A holder refuses an offer
/// whose flags do not fit the input, so a tool's command line never drifts from its input ([`Offered::BadFlags`]).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.tool_sheet")]
pub struct ToolSheet {
    pub name: String,
    pub description: String,
    pub input: SchemaNodes,
    /// The input's fields a command line fills, in declared order, as its `#[flag]`s mark them.
    pub flags: Vec<ToolFlag>,
    pub output: Output,
}

impl ToolSheet {
    /// The sheet of the tool `name`, doing what `description` says, whose input is `I`, typed with the flags `I`'s
    /// fields mark (ADR-0053), and answering with `output`.
    #[must_use]
    pub fn of<I: Flagged>(name: &str, description: &str, output: Output) -> Self {
        let flags = I::FIELDS.iter().filter_map(ToolFlag::of).collect();

        Self { flags, ..Self::json::<I>(name, description, output) }
    }

    /// The sheet of the tool `name`, doing what `description` says, whose input is `I`, typed as one JSON word, or
    /// none for an input of no fields; and answering with `output`.
    #[must_use]
    pub fn json<I: Schema>(name: &str, description: &str, output: Output) -> Self {
        Self {
            name: String::from(name),
            description: String::from(description),
            input: SchemaNodes::of(&I::SHAPE),
            flags: Vec::new(),
            output,
        }
    }
}

/// One flag of a tool's command line: the input's field it fills, how it is typed, and the field's doc comment, its
/// help (ADR-0053). Plain data, so it crosses the link with the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.tool_flag")]
pub struct ToolFlag {
    /// The name of the input's field.
    pub field: String,
    /// How the flag is typed.
    pub typed: Typed,
    /// The field's doc comment.
    pub doc: String,
}

impl ToolFlag {
    /// The flag `declared` suggests: none for a field with no `#[flag]`.
    fn of(declared: &Declared) -> Option<Self> {
        let typed = match declared.flag {
            Suggested::NoFlag => return None,
            Suggested::Position => Typed::Position,
            Suggested::Long { long } => Typed::Long { long: String::from(long) },
            Suggested::LongAndShort { long, short } => Typed::LongAndShort { long: String::from(long), short },
        };

        Some(Self { field: String::from(declared.name), typed, doc: String::from(declared.doc) })
    }
}

/// How a tool's flag is typed (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.typed")]
pub enum Typed {
    /// As the next bare word, in declared order.
    Position,
    /// As `--long value`, or `--long=value`; a switch alone, or `--no-long`.
    Long { long: String },
    /// As `--long`, or as `-short value`.
    LongAndShort { long: String, short: char },
}

/// What a tool answers with (ADR-0043).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.output")]
pub enum Output {
    /// A value of the schema: [`Called::Output`].
    Value(SchemaNodes),
    /// An image: [`Called::Image`].
    Image,
    /// Either: a value of the schema or an image, as the hub's `call_tool` answers what an engine's tool answered.
    ValueOrImage(SchemaNodes),
}

/// Tools to hold, all or none, and the worker each call of them is asked of.
#[derive(Debug, Bound)]
#[kind(name = "aether.tools.offer")]
pub struct Offer {
    pub tools: Vec<ToolSheet>,
    pub worker: ProtocolRef<Tooling>,
}

/// What an offer came to. Anything but `Yes` holds none of its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.offered")]
pub enum Offered {
    /// Held.
    Yes,
    /// A name is already held, or is in the offer twice.
    Taken,
    /// It would take the holder past its most tools.
    Full,
    /// An input's JSON form is not an object, as a call's arguments are.
    NotAnObject,
    /// A name is outside what MCP asks of one: 1 to 128 of ASCII letters, digits, `_`, `-`, and `.`.
    BadName,
    /// A schema it lists has no JSON Schema: it is too deep or too large, or a map's key is not a key shape.
    Unwritable,
    /// A tool's flags do not fit its input (ADR-0053): a flag names a field the input lacks, two flags share a name
    /// or a field, a field read by position is a switch or must be given after one that may be left out, or a field
    /// that must be given has no flag.
    BadFlags,
}

/// A tool to stop holding, when `worker` is the worker that holds it: another's of the same name stays.
#[derive(Debug, Clone, Bound)]
#[kind(name = "aether.tools.withdraw")]
pub struct Withdraw {
    pub tool: String,
    pub worker: ProtocolRef<Tooling>,
}
