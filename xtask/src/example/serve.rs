//! A built page served over http on localhost, where a browser lets WebGPU run: the files under its `pkg/`, one
//! request at a time, until the process is stopped.

use crate::failure::Failure;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// How long a connection may take to send its request before it is dropped, so one that sends nothing cannot hold up
/// the next.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Serves `pkg` on `port` of localhost, port 0 for one the system picks, printing `serving example=<name>
/// url=<the page's address>` once it listens. It serves until the process is stopped.
pub fn serve(pkg: &Path, name: &str, port: u16) -> Result<(), Failure> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let address = listener.local_addr()?;
    println!("serving example={name} url=http://{address}/");

    for stream in listener.incoming() {
        let answered = stream.and_then(|stream| answer(pkg, &stream));
        if let Err(error) = answered {
            eprintln!("dropped request: {error}");
        }
    }
    Ok(())
}

/// Reads one request from `stream` and answers it, then closes it.
fn answer(pkg: &Path, stream: &TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // The headers say nothing a static file needs; they are read to their blank line, so the request is whole.
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim_end().is_empty() {
            break;
        }
    }

    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let response = match (method, resolve(target)) {
        ("GET" | "HEAD", Target::File(relative)) => file(&pkg.join(relative)),
        ("GET" | "HEAD", Target::Escapes) => Response::status("400 Bad Request"),
        _ => Response::status("405 Method Not Allowed"),
    };
    response.write(stream, method == "HEAD")
}

/// Where a request's target points, under the served directory.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// A file's path, relative to the served directory.
    File(PathBuf),
    /// A path that would leave the served directory, refused.
    Escapes,
}

/// The file a request's target names, relative to the served directory: its path without query or fragment, `/`
/// itself the page. A path with a `..`, or any part that is not a plain name, escapes.
fn resolve(target: &str) -> Target {
    let path = target.split_once(['?', '#']).map_or(target, |(path, _)| path);
    let relative = match path.trim_start_matches('/') {
        "" => "index.html",
        relative => relative,
    };
    let plain = Path::new(relative).components().all(|component| matches!(component, Component::Normal(_)));
    if plain {
        Target::File(PathBuf::from(relative))
    } else {
        Target::Escapes
    }
}

/// The file at `path`, typed by its extension, or a 404 where there is none.
fn file(path: &Path) -> Response {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    // A module is compiled as it streams in only when served as `application/wasm`.
    let content_type = match extension {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    };
    fs::read(path)
        .map_or_else(|_| Response::status("404 Not Found"), |body| Response { status: "200 OK", content_type, body })
}

/// One answer, written whole and the connection closed.
struct Response {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    /// An answer with no file: its status, as plain text.
    fn status(status: &'static str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: status.as_bytes().to_vec() }
    }

    /// Writes the answer, its body left off for a `HEAD`. Nothing is cached, so a page built again is the page shown.
    fn write(&self, mut stream: &TcpStream, head: bool) -> io::Result<()> {
        let Self { status, content_type, body } = self;
        let length = body.len();
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nCache-Control: no-store\r\n\
             Connection: close\r\n\r\n"
        )?;
        if !head {
            stream.write_all(body)?;
        }
        stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::{Target, resolve};
    use std::path::PathBuf;

    #[test]
    fn the_root_is_the_page_and_a_query_is_left_off() {
        assert_eq!(resolve("/"), Target::File(PathBuf::from("index.html")));
        assert_eq!(resolve("/draw_bg.wasm?v=2"), Target::File(PathBuf::from("draw_bg.wasm")));
        assert_eq!(resolve("/snippets/a/b.js#x"), Target::File(PathBuf::from("snippets/a/b.js")));
    }

    #[test]
    fn a_path_out_of_the_served_directory_is_refused() {
        assert_eq!(resolve("/../Cargo.toml"), Target::Escapes);
        assert_eq!(resolve("/snippets/../../lib.rs"), Target::Escapes);
        assert_eq!(resolve("//.."), Target::Escapes);
    }
}
