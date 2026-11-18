//! Rust sources read the way the checks need them: comments blanked out of the code, and the text of doc comments,
//! whose examples run as doctests, kept apart.
//!
//! Blanking replaces bytes with spaces rather than removing them, so an offset into the blanked code is an offset into
//! the file and [`RustSource::line_of`] turns a match back into the line a failure names.

use std::fs;
use std::iter;
use std::path::{Path, PathBuf};

/// One `.rs` file, read for scanning.
pub struct RustSource {
    /// The file it was read from, absolute.
    pub path: PathBuf,
    /// The file with every comment byte a space.
    pub code: String,
    /// The file with every byte outside a doc comment's text a space: what a doctest is built from.
    pub docs: String,
}

impl RustSource {
    /// Reads `path`; `None` when it is not UTF-8 text.
    pub fn read(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        Some(Self { path: path.to_path_buf(), code: blank_comments(&text), docs: doc_text(&text) })
    }

    /// The 1-based line holding `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        let mut end = offset.min(self.code.len());
        while !self.code.is_char_boundary(end) {
            end -= 1;
        }
        self.code[..end].matches('\n').count() + 1
    }
}

/// What a walk of a directory found: every `.rs` file, every `Cargo.toml` and every file of any kind, and every
/// symlink, never followed. `target/` and `.git/` are skipped: build output and object storage are not source.
pub struct Walk {
    pub rust_files: Vec<PathBuf>,
    pub manifests: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub symlinks: Vec<PathBuf>,
}

/// Walks `root` with a stack of its own, never by recursion, so no depth of tree overflows.
pub fn walk(root: &Path) -> Walk {
    let mut found = Walk { rust_files: Vec::new(), manifests: Vec::new(), files: Vec::new(), symlinks: Vec::new() };
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                found.symlinks.push(path);
            } else if kind.is_dir() {
                let skipped = matches!(entry.file_name().to_str(), Some("target" | ".git"));
                if !skipped {
                    stack.push(path);
                }
            } else {
                if path.extension().is_some_and(|extension| extension == "rs") {
                    found.rust_files.push(path.clone());
                }
                if entry.file_name() == "Cargo.toml" {
                    found.manifests.push(path.clone());
                }
                found.files.push(path);
            }
        }
    }
    found.rust_files.sort();
    found.manifests.sort();
    found.files.sort();
    found.symlinks.sort();
    found
}

/// `text` with every comment byte a space, string and character literals untouched.
fn blank_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = literal_end(bytes, index) {
            index = end;
        } else if bytes[index..].starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                out[index] = b' ';
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            index = blank_block_comment(bytes, &mut out, index);
        } else {
            index += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The text of `text`'s line doc comments (`///` and `//!`), each line's text where it stood and every other byte a
/// space.
fn doc_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        let documented = body.starts_with("///") || body.starts_with("//!");
        if documented {
            out.extend(iter::repeat_n(' ', indent + 3));
            out.push_str(&body[3..]);
        } else {
            // Byte for byte, so an offset into the doc text is an offset into the file.
            let blank = line.len() - usize::from(line.ends_with('\n'));
            out.extend(iter::repeat_n(' ', blank));
            if line.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

/// Blanks the block comment, nested ones in it, opening at `start`, and answers with the offset just past it.
fn blank_block_comment(bytes: &[u8], out: &mut [u8], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut index = start;
    while index < bytes.len() {
        let opening = bytes[index..].starts_with(b"/*");
        let closing = bytes[index..].starts_with(b"*/");
        if opening || closing {
            depth = if opening {
                depth + 1
            } else {
                depth - 1
            };
            out[index] = b' ';
            out[index + 1] = b' ';
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            if bytes[index] != b'\n' {
                out[index] = b' ';
            }
            index += 1;
        }
    }
    index
}

/// The end of the literal starting at `start`, or `None` where none starts: a `'` opening a lifetime, or an `r` that is
/// an identifier's.
fn literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes[start] {
        b'"' => Some(quoted_literal_end(bytes, start + 1)),
        b'\'' => char_literal_end(bytes, start),
        b'r' if !preceded_by_identifier(bytes, start) => raw_string_span(bytes, start).map(|span| span.end),
        _ => None,
    }
}

/// The end of a `"`-quoted literal whose body starts at `body`.
fn quoted_literal_end(bytes: &[u8], body: usize) -> usize {
    let mut cursor = body;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'"' => return cursor + 1,
            _ => cursor += 1,
        }
    }
    bytes.len()
}

/// Where a raw string's body, and the literal, end.
struct LiteralSpan {
    body: usize,
    body_end: usize,
    end: usize,
}

/// The raw string opening at `start`, or `None` where the `r` opens none.
fn raw_string_span(bytes: &[u8], start: usize) -> Option<LiteralSpan> {
    let mut cursor = start + 1;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }

    let terminator: Vec<u8> = iter::once(b'"').chain(iter::repeat_n(b'#', cursor - start - 1)).collect();
    let body = cursor + 1;
    let body_end = (body..bytes.len()).find(|offset| bytes[*offset..].starts_with(&terminator)).unwrap_or(bytes.len());
    Some(LiteralSpan { body, body_end, end: (body_end + terminator.len()).min(bytes.len()) })
}

/// The bodies of the string literals in `code`, each with the offset it starts at. A raw string's body is verbatim; an
/// escaped one's is as written.
pub fn string_literals(code: &str) -> Vec<(usize, &str)> {
    let bytes = code.as_bytes();
    let mut literals = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => {
                let end = quoted_literal_end(bytes, cursor + 1);
                push_body(code, cursor + 1, end.saturating_sub(1), &mut literals);
                cursor = end;
            }
            b'\'' => cursor = char_literal_end(bytes, cursor).unwrap_or(cursor + 1),
            b'r' if !preceded_by_identifier(bytes, cursor) => match raw_string_span(bytes, cursor) {
                Some(span) => {
                    push_body(code, span.body, span.body_end, &mut literals);
                    cursor = span.end;
                }
                None => cursor += 1,
            },
            _ => cursor += 1,
        }
    }
    literals
}

/// Records one literal's body, unless an unterminated literal left it straddling a multi-byte character.
fn push_body<'code>(code: &'code str, body: usize, body_end: usize, literals: &mut Vec<(usize, &'code str)>) {
    let whole = body <= body_end && code.is_char_boundary(body) && code.is_char_boundary(body_end);
    if whole {
        literals.push((body, &code[body..body_end]));
    }
}

/// The end of a character literal, or `None` for a lifetime: a character literal closes within a few bytes (`'x'`,
/// `'\n'`, `'\u{1f600}'`), and a lifetime never does.
fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    const LONGEST: usize = 12;
    let mut cursor = start + 1;
    if bytes.get(cursor) == Some(&b'\\') {
        cursor += 1;
    }
    let limit = (start + LONGEST).min(bytes.len());
    (cursor..limit).find(|offset| bytes[*offset] == b'\'').map(|offset| offset + 1)
}

/// Whether the byte before `index` continues an identifier; `b`, a byte string's prefix (`br"…"`), does not count.
fn preceded_by_identifier(bytes: &[u8], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).map(|before| bytes[before]) else {
        return false;
    };
    previous != b'b' && (previous.is_ascii_alphanumeric() || previous == b'_')
}

#[cfg(test)]
mod tests {
    use super::{blank_comments, doc_text, string_literals};

    #[test]
    fn blanking_keeps_offsets_and_literals_and_docs_keep_only_doc_text() {
        // Catches a blanker that moves a byte, which sends a failure to the wrong line, one that eats a `//` inside a
        // string, which loses the path a check reads, and doc text that keeps code, which scans a file twice.
        let text = "let path = \"a//b\"; // \"comment\"\n/// \"doc/path\"\nlet raw = r#\"x /* y */ z\"#;\n";
        let code = blank_comments(text);
        let docs = doc_text(text);

        assert_eq!(code.len(), text.len());
        assert_eq!(docs.lines().count(), text.lines().count());
        let in_code: Vec<&str> = string_literals(&code).into_iter().map(|(_, literal)| literal).collect();
        assert_eq!(in_code, ["a//b", "x /* y */ z"]);
        let in_docs: Vec<&str> = string_literals(&docs).into_iter().map(|(_, literal)| literal).collect();
        assert_eq!(in_docs, ["doc/path"]);
    }
}
