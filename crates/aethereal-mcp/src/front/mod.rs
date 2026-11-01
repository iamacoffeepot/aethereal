//! The front of the root: what a request is and what it is answered, with no actor in it (ADR-0037).

mod base64;
mod origin;
mod reply;
mod table;
#[cfg(test)]
mod tests;

pub use reply::{output, refused};
pub use table::{Table, Tool};

use aethereal_data::{Budget, JsonTree, JsonValue, from_json, from_value, read_json};
use aethereal_http::{Method, Request, Response, Status};
use core::str;

/// The revision spoken, the only one (ADR-0037).
const VERSION: &str = "2026-07-28";
/// The most bytes a request's JSON may take to read. `Http` bounds the body before this.
const READ_BYTES: usize = 256 * 1024 * 1024;

/// `-32601`: no such method.
const METHOD_NOT_FOUND: i32 = -32601;
/// `-32602`: the params are wrong.
const INVALID_PARAMS: i32 = -32602;
/// `-32020`: a header is missing, malformed, or disagrees with the body.
const HEADER_MISMATCH: i32 = -32020;

/// Where a request goes.
pub enum Routed {
    /// It is answered now.
    Respond(Response),
    /// It is a call of a tool held, with its input read: the worker answers it.
    Call { id: String, tool: String, input: Vec<u8> },
}

/// What `request` is, each row of ADR-0037's table in its order. `origins` are the origins allowed besides this
/// machine's.
pub fn route(request: &Request, origins: &[String], table: &Table) -> Routed {
    let is_allowed = request.header("origin").is_none_or(|origin| origin::is_allowed(origin, origins));
    if !is_allowed {
        return Routed::Respond(Response::empty(Status::Forbidden));
    }
    if request.path != "/mcp" {
        return Routed::Respond(Response::empty(Status::NotFound));
    }
    if request.method != Method::Post {
        return Routed::Respond(Response::empty(Status::MethodNotAllowed));
    }
    let Ok(text) = str::from_utf8(&request.body) else {
        return Routed::Respond(Response::empty(Status::BadRequest));
    };
    let mut budget = Budget::new(READ_BYTES);
    let Ok(tree) = read_json(text, &mut budget) else {
        return Routed::Respond(Response::empty(Status::BadRequest));
    };
    let Some(message) = Message::of(&tree) else {
        return Routed::Respond(Response::empty(Status::BadRequest));
    };
    let Message { id, method, params } = message;
    let Id::Of(id) = id else {
        return Routed::Respond(Response::empty(Status::Accepted));
    };
    let call = Rpc { request, tree: &tree, id, method, params };

    match call.checked() {
        Err(response) => Routed::Respond(response),
        Ok(()) => call.dispatch(table, &mut budget),
    }
}

/// A message's `id`: a notification has none.
enum Id {
    Absent,
    /// Its JSON text.
    Of(String),
}

/// The envelope of one JSON-RPC request or notification.
struct Message<'tree, 'json> {
    id: Id,
    method: &'tree str,
    params: Option<&'tree JsonValue<'json>>,
}

impl<'tree, 'json> Message<'tree, 'json> {
    /// The message `tree` holds: none when it is no request or notification, as a batch, a response, or a null `id`
    /// is not.
    fn of(tree: &'tree JsonTree<'json>) -> Option<Self> {
        let JsonValue::Object(root) = tree.root() else {
            return None;
        };
        let Some(JsonValue::String(version)) = tree.member(*root, "jsonrpc") else {
            return None;
        };
        if version != "2.0" {
            return None;
        }
        let Some(JsonValue::String(method)) = tree.member(*root, "method") else {
            return None;
        };
        let id = match tree.member(*root, "id") {
            None => Id::Absent,
            Some(id) => Id::Of(id_text(id)?),
        };

        Some(Self { id, method, params: tree.member(*root, "params") })
    }
}

/// The JSON text of `id`: none when it is neither a string nor an integer.
fn id_text(id: &JsonValue<'_>) -> Option<String> {
    match id {
        JsonValue::String(text) => {
            let mut out = String::new();
            aethereal_data::write_json_string(text, &mut out);
            Some(out)
        }
        JsonValue::Number(text) => {
            let is_integer = text.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-');

            is_integer.then(|| String::from(*text))
        }
        _ => None,
    }
}

/// A request to answer.
struct Rpc<'call, 'json> {
    request: &'call Request,
    tree: &'call JsonTree<'json>,
    id: String,
    method: &'call str,
    params: Option<&'call JsonValue<'json>>,
}

impl<'call, 'json> Rpc<'call, 'json> {
    /// The member `name` of the params: none when it is missing, or the params are no object.
    fn param(&self, name: &str) -> Option<&'call JsonValue<'json>> {
        let Some(JsonValue::Object(params)) = self.params else {
            return None;
        };

        self.tree.member(*params, name)
    }

    /// The member `name` of `_meta`: none when it is missing.
    fn meta(&self, name: &str) -> Option<&'call JsonValue<'json>> {
        let Some(JsonValue::Object(meta)) = self.param("_meta") else {
            return None;
        };

        self.tree.member(*meta, name)
    }

    /// The string the param `name` holds: none when it holds none.
    fn text_param(&self, name: &str) -> Option<&'call str> {
        let Some(JsonValue::String(text)) = self.param(name) else {
            return None;
        };

        Some(text)
    }

    /// The error of `code`, sent with `status`.
    fn error(&self, status: Status, code: i32, message: &str) -> Response {
        reply::error(status, &self.id, code, message)
    }

    /// Answers a request that its version, headers, or `_meta` refuse.
    fn checked(&self) -> Result<(), Response> {
        if self.method == "initialize" {
            let requested = self.text_param("protocolVersion").unwrap_or_default();
            return Err(reply::unsupported(&self.id, requested));
        }
        let Some(version) = self.request.header("mcp-protocol-version") else {
            return Err(self.mismatch("`MCP-Protocol-Version` is missing"));
        };
        if version != VERSION {
            return Err(reply::unsupported(&self.id, version));
        }
        self.headers_match(version)?;
        self.meta_is_whole()
    }

    /// The error for a header that does not hold.
    fn mismatch(&self, message: &str) -> Response {
        self.error(Status::BadRequest, HEADER_MISMATCH, message)
    }

    /// Refuses a header missing, or one that disagrees with the body.
    fn headers_match(&self, version: &str) -> Result<(), Response> {
        let method = self.request.header("mcp-method");
        if method != Some(self.method) {
            return Err(self.mismatch("`Mcp-Method` does not match the body"));
        }
        let stated = self.meta("io.modelcontextprotocol/protocolVersion");
        if let Some(JsonValue::String(stated)) = stated
            && stated != version
        {
            return Err(self.mismatch("`MCP-Protocol-Version` does not match the body"));
        }
        if self.method != "tools/call" {
            return Ok(());
        }
        let header = self.request.header("mcp-name").and_then(decoded_name);
        let name = self.text_param("name");
        let is_both_absent = header.is_none() && name.is_none();
        let is_same = header.as_deref() == name;
        if is_both_absent || is_same {
            return Ok(());
        }

        Err(self.mismatch("`Mcp-Name` does not match the body"))
    }

    /// Refuses `_meta` without what every request must carry.
    fn meta_is_whole(&self) -> Result<(), Response> {
        let version = self.meta("io.modelcontextprotocol/protocolVersion");
        let capabilities = self.meta("io.modelcontextprotocol/clientCapabilities");
        let has_version = matches!(version, Some(JsonValue::String(_)));
        let has_capabilities = matches!(capabilities, Some(JsonValue::Object(_)));
        if has_version && has_capabilities {
            return Ok(());
        }

        Err(self.error(Status::BadRequest, INVALID_PARAMS, "`_meta` is missing a required field"))
    }

    /// Answers a request that was checked.
    fn dispatch(&self, table: &Table, budget: &mut Budget) -> Routed {
        let response = match self.method {
            "server/discover" => reply::discover(&self.id),
            "tools/list" if self.param("cursor").is_some() => {
                self.error(Status::Ok, INVALID_PARAMS, "this server issues no cursors")
            }
            "tools/list" => reply::list(&self.id, table),
            "tools/call" => return self.call(table, budget),
            _ => self.error(Status::NotFound, METHOD_NOT_FOUND, "no such method"),
        };

        Routed::Respond(response)
    }

    /// Reads a call of a tool.
    fn call(&self, table: &Table, budget: &mut Budget) -> Routed {
        let Some(name) = self.text_param("name") else {
            return Routed::Respond(self.error(Status::Ok, INVALID_PARAMS, "`name` is missing"));
        };
        let Some(tool) = table.tool(name) else {
            return Routed::Respond(self.error(Status::Ok, INVALID_PARAMS, "no such tool"));
        };
        let arguments = self.param("arguments");
        let read = arguments.map_or_else(
            || from_json(&tool.input, "{}", budget),
            |arguments| from_value(&tool.input, self.tree, arguments),
        );
        match read {
            Ok(input) => Routed::Call { id: self.id.clone(), tool: String::from(name), input },
            Err(fault) => Routed::Respond(refused(&self.id, &format!("the arguments: {fault}"))),
        }
    }
}

/// The name an `Mcp-Name` header holds, decoded when it is in the Base64 form: none when that form does not decode.
fn decoded_name(value: &str) -> Option<String> {
    let encoded = value.strip_prefix("=?base64?").and_then(|rest| rest.strip_suffix("?="));
    let Some(encoded) = encoded else {
        return Some(String::from(value));
    };

    String::from_utf8(base64::decode(encoded)?).ok()
}
