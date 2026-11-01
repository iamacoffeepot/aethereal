//! The JSON a request is answered with (ADR-0037).

use super::{Table, VERSION};
use aethereal_data::{Budget, SchemaNodes, to_json, write_json_string};
use aethereal_http::{Response, Status};

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

/// A response of JSON-RPC text.
fn json(status: Status, body: String) -> Response {
    Response::new(status, "application/json", body.into_bytes())
}

/// The response to request `id` whose result, without its common fields, is `fields`.
fn result(id: &str, fields: &str) -> Response {
    let body = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"resultType\":\"complete\",{fields},{SERVER_INFO}}}}}"
    );

    json(Status::Ok, body)
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::new();
    write_json_string(text, &mut out);

    out
}

/// The JSON-RPC error `code` to request `id`, sent with `status`.
pub fn error(status: Status, id: &str, code: i32, message: &str) -> Response {
    let message = quoted(message);
    let body = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{message}}}}}");

    json(status, body)
}

/// The error for a revision not spoken, naming the one that is.
pub fn unsupported(id: &str, requested: &str) -> Response {
    let requested = quoted(requested);
    let data = format!("{{\"supported\":[\"{VERSION}\"],\"requested\":{requested}}}");
    let body = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{UNSUPPORTED_VERSION},\
         \"message\":\"Unsupported protocol version\",\"data\":{data}}}}}"
    );

    json(Status::BadRequest, body)
}

/// The answer to `server/discover`.
pub fn discover(id: &str) -> Response {
    result(id, &format!("\"supportedVersions\":[\"{VERSION}\"],\"capabilities\":{{\"tools\":{{}}}},{FRESHNESS}"))
}

/// The answer to `tools/list`.
pub fn list(id: &str, table: &Table) -> Response {
    let tools = table.listed().collect::<Vec<_>>().join(",");

    result(id, &format!("\"tools\":[{tools}],{FRESHNESS}"))
}

/// A tool's entry in `tools/list`, its schemas already JSON Schema.
pub fn listed(name: &str, description: &str, input: &str, output: &str) -> String {
    let (name, description) = (quoted(name), quoted(description));

    format!("{{\"name\":{name},\"description\":{description},\"inputSchema\":{input},\"outputSchema\":{output}}}")
}

/// The result of a tool that could not do it, which says why.
pub fn refused(id: &str, why: &str) -> Response {
    let text = quoted(why);

    result(id, &format!("\"content\":[{{\"type\":\"text\",\"text\":{text}}}],\"isError\":true"))
}

/// The result of a tool that answered `bytes`, a value of `schema` by its word.
pub fn output(id: &str, schema: &SchemaNodes, bytes: &[u8]) -> Response {
    let Ok(value) = to_json(schema, bytes, &mut Budget::new(OUTPUT_BYTES)) else {
        return refused(id, "the tool's output is no value of its schema");
    };
    let text = quoted(&value);

    result(
        id,
        &format!("\"content\":[{{\"type\":\"text\",\"text\":{text}}}],\"structuredContent\":{value},\"isError\":false"),
    )
}
