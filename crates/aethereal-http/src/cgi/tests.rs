use super::{CgiHead, head};
use crate::stack::ResponseHead;
use crate::{Header, Status};

#[test]
fn a_status_line_names_the_status_and_its_header_is_taken_out_and_the_body_starts_past_the_empty_line() {
    // Catches `Status` passed on as a header of the response, written to the client beside the status line it should
    // have set; a response answered `200` whatever its program said, so a refusal reads as a success; and the bytes of
    // the body that came with the head counted as the head's, or dropped.
    let output = b"Status: 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nrefused";

    let read = head(output, 1024);

    let head = ResponseHead { status: Status::Forbidden, headers: vec![Header::new("content-type", "text/plain")] };
    assert_eq!(read, CgiHead::Whole { head, consumed: output.len() - "refused".len() });
}

#[test]
fn a_head_with_no_status_is_ok() {
    // Catches a head that names no status read as malformed, where a CGI program's success says no status.
    let output = b"Content-Type: application/x-git-upload-pack-advertisement\r\n\r\n";

    let read = head(output, 1024);

    let headers = vec![Header::new("content-type", "application/x-git-upload-pack-advertisement")];
    assert_eq!(read, CgiHead::Whole { head: ResponseHead { status: Status::Ok, headers }, consumed: output.len() });
}

#[test]
fn a_head_not_yet_ended_wants_more_and_one_past_its_most_bytes_is_too_large() {
    // Catches a head cut where a read cut it, so a header split between two of the program's writes is lost, and a
    // program that writes no empty line held without bound.
    let unended = b"Status: 200 OK\r\nContent-Type: text/plain\r\n";

    assert_eq!(head(unended, 1024), CgiHead::More);
    assert_eq!(head(unended, 16), CgiHead::TooLarge);
}

#[test]
fn a_status_that_names_no_status_and_a_status_named_twice_are_malformed() {
    // Catches a code no response is written with passed on, which the stack could not write, a code read from its
    // first digits alone, a second `Status` that one reader would take and another pass over, and a line feed without
    // its carriage return, which would let a header end where another reader would not end it.
    let unknown = head(b"Status: 299 Odd\r\n\r\n", 1024);
    let long = head(b"Status: 2000\r\n\r\n", 1024);
    let twice = head(b"Status: 200 OK\r\nStatus: 403 Forbidden\r\n\r\n", 1024);
    let bare = head(b"Status: 200 OK\nContent-Type: text/plain\r\n\r\n", 1024);

    assert_eq!(unknown, CgiHead::Malformed);
    assert_eq!(long, CgiHead::Malformed);
    assert_eq!(twice, CgiHead::Malformed);
    assert_eq!(bare, CgiHead::Malformed);
}
