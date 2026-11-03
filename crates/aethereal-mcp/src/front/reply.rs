//! The JSON a request is answered with (ADR-0037).

use super::{VERSION, base64};
use aethereal_data::{Budget, SchemaNodes, to_json, write_json_string};
use aethereal_http::Status;
use aethereal_tools::{HeldOutput, HeldTool, Table};

/// `-32022`: the revision asked for is not spoken.
const UNSUPPORTED_VERSION: i32 = -32022;
/// The most bytes of JSON a tool's output may write.
const OUTPUT_BYTES: usize = 256 * 1024 * 1024;
/// What every result says of who answered it.
const SERVER_INFO: &str = concat!(
    "\"_meta\":{\"io.modelcontextprotocol/serverInfo\":{\"name\":\"aethereal\",\"version\":\"",
    env!("CARGO_PKG_VERSION"),
    "\"}}"
);
/// What a result that holds a list says of how long it keeps: not at all, as tools come and go, and the same for every
/// caller.
const FRESHNESS: &str = "\"ttlMs\":0,\"cacheScope\":\"public\"";
/// Room enough for the fixed text a result is framed in, past its id and its own fields.
const FRAME_BYTES: usize = SERVER_INFO.len() + 128;

/// What a request is answered with, before its body is put as a blob by the actor that answers it (ADR-0042).
#[derive(Debug)]
pub enum Reply {
    /// `status`, with no body.
    Empty(Status),
    /// JSON-RPC text, sent with `status`.
    Json { status: Status, body: String },
}

/// A reply of JSON-RPC text.
const fn json(status: Status, body: String) -> Reply {
    Reply::Json { status, body }
}

/// The reply to request `id` whose result, without its common fields, is `fields`.
fn result(id: &str, fields: &str) -> Reply {
    let mut body = opened(id, fields.len());
    body.push_str(fields);

    closed(body)
}

/// A reply to request `id` up to its result's own fields, with room for `fields` bytes of them, written straight
/// into it so that a large result is not copied again.
fn opened(id: &str, fields: usize) -> String {
    opened_in(String::with_capacity(fields + id.len() + FRAME_BYTES), id)
}

/// A reply to request `id` up to its result's own fields, written into `body`, which is empty.
fn opened_in(mut body: String, id: &str) -> String {
    body.push_str("{\"jsonrpc\":\"2.0\",\"id\":");
    body.push_str(id);
    body.push_str(",\"result\":{\"resultType\":\"complete\",");

    body
}

/// The reply that is `body`, [`opened`] with its result's own fields written after, closed.
fn closed(mut body: String) -> Reply {
    body.push(',');
    body.push_str(SERVER_INFO);
    body.push_str("}}");

    json(Status::Ok, body)
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::new();
    write_json_string(text, &mut out);

    out
}

/// The JSON-RPC error `code` to request `id`, sent with `status`.
pub fn error(status: Status, id: &str, code: i32, message: &str) -> Reply {
    let message = quoted(message);
    let body = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{message}}}}}");

    json(status, body)
}

/// The error for a revision not spoken, naming the one that is.
pub fn unsupported(id: &str, requested: &str) -> Reply {
    let requested = quoted(requested);
    let data = format!("{{\"supported\":[\"{VERSION}\"],\"requested\":{requested}}}");
    let body = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{UNSUPPORTED_VERSION},\
         \"message\":\"Unsupported protocol version\",\"data\":{data}}}}}"
    );

    json(Status::BadRequest, body)
}

/// The answer to `server/discover`.
pub fn discover(id: &str) -> Reply {
    result(id, &format!("\"supportedVersions\":[\"{VERSION}\"],\"capabilities\":{{\"tools\":{{}}}},{FRESHNESS}"))
}

/// The answer to `tools/list`.
pub fn list(id: &str, table: &Table) -> Reply {
    let tools = table.tools().map(|(name, held)| listed(name, held)).collect::<Vec<_>>().join(",");

    result(id, &format!("\"tools\":[{tools}],{FRESHNESS}"))
}

/// A tool's entry in `tools/list`, from the JSON Schema written as it was offered. Only a tool that answers a value
/// lists an output schema, as `structuredContent` must match one listed and an image has none (ADR-0043).
fn listed(name: &str, held: &HeldTool) -> String {
    let (name, description, input) = (quoted(name), quoted(&held.description), &held.input_schema);
    let output = match &held.output {
        HeldOutput::Value { json_schema, .. } => format!(",\"outputSchema\":{json_schema}"),
        HeldOutput::Image | HeldOutput::ValueOrImage(_) => String::new(),
    };

    format!("{{\"name\":{name},\"description\":{description},\"inputSchema\":{input}{output}}}")
}

/// The result of a tool that could not do it, which says why.
pub fn refused(id: &str, why: &str) -> Reply {
    let text = quoted(why);

    result(id, &format!("\"content\":[{{\"type\":\"text\",\"text\":{text}}}],\"isError\":true"))
}

/// The JSON of a tool's output, `bytes` a value of `schema` by its word, which the agent reads: refused, as the reply to
/// request `id`, when it is no value of it. The tool's bytes are read where they lie (ADR-0042).
pub fn value(id: &str, schema: &SchemaNodes, bytes: &[u8]) -> Result<String, Reply> {
    to_json(schema, bytes, &mut Budget::new(OUTPUT_BYTES))
        .map_err(|_| refused(id, "the tool's output is no value of its schema"))
}

/// The room the result to request `id` of a tool whose output's JSON is `value` takes: the text is the value escaped
/// once more, as long as it and longer by an escape for each quote and backslash, beside the value itself.
pub fn output_room(id: &str, value: &str) -> usize {
    2 * value.len() + id.len() + 2 * FRAME_BYTES
}

/// The room the result to request `id` of a tool that answered an image of `png` bytes with `text` takes: the image in
/// Base64, and the text escaped, longer by an escape for each quote and backslash.
pub fn image_room(id: &str, png: usize, text: &str) -> usize {
    base64::encoded_len(png) + 2 * text.len() + id.len() + 2 * FRAME_BYTES
}

/// The result to request `id` of a tool that answered the image `png` with `text`, written into `buffer`, which has
/// the room [`image_room`] says: an image block, then a text block, and no `structuredContent` (ADR-0043).
pub fn image(id: &str, png: &[u8], text: &str, mut buffer: Vec<u8>) -> Reply {
    buffer.clear();
    // An empty buffer is always text.
    let mut body = opened_in(String::from_utf8(buffer).unwrap_or_default(), id);
    body.push_str("\"content\":[{\"type\":\"image\",\"data\":\"");
    base64::encode(png, &mut body);
    body.push_str("\",\"mimeType\":\"image/png\"},{\"type\":\"text\",\"text\":");
    write_json_string(text, &mut body);
    body.push_str("}],\"isError\":false");

    closed(body)
}

/// The result to request `id` of a tool whose output's JSON is `value`, written into `buffer`, which has the room
/// [`output_room`] says: one the actor that answers it keeps, so a large result is built in memory already in use.
pub fn output(id: &str, value: &str, mut buffer: Vec<u8>) -> Reply {
    buffer.clear();
    // An empty buffer is always text.
    let mut body = opened_in(String::from_utf8(buffer).unwrap_or_default(), id);
    body.push_str("\"content\":[{\"type\":\"text\",\"text\":");
    write_json_string(value, &mut body);
    body.push_str("}],\"structuredContent\":");
    body.push_str(value);
    body.push_str(",\"isError\":false");

    closed(body)
}
