//! Cutting a path's text at a mark (ADR-0008).

/// The bytes before the first `mark`, and the bytes after it when `bytes` holds one.
pub(super) const fn cut(bytes: &[u8], mark: u8) -> (&[u8], Option<&[u8]>) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == mark {
            let (before, marked) = bytes.split_at(index);
            let (_, after) = marked.split_at(1);
            return (before, Some(after));
        }
        index += 1;
    }

    (bytes, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_is_at_the_first_mark_and_drops_it() {
        // Catches a cut at the last mark, one that keeps the mark, and one that loses an empty side.
        assert_eq!(cut(b"a/b/c", b'/'), (&b"a"[..], Some(&b"b/c"[..])));
        assert_eq!(cut(b"/b", b'/'), (&b""[..], Some(&b"b"[..])));
        assert_eq!(cut(b"a/", b'/'), (&b"a"[..], Some(&b""[..])));
        assert_eq!(cut(b"abc", b'/'), (&b"abc"[..], None));
    }
}
