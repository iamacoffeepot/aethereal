//! What a stack and a client share of HTTP/1.1 (RFC 9110, RFC 9112): its methods, headers, and statuses; where a head
//! ends and how its lines and headers parse, with every limit checked before anything goes further; how a body is
//! framed and decoded; and whether an engine can carry what either takes in. A stack reads requests and writes
//! responses with these, and a client writes requests and reads responses.

pub mod body;
pub mod fit;
pub mod head;
mod header;
mod method;
mod status;

pub use header::Header;
pub use method::Method;
pub use status::Status;
