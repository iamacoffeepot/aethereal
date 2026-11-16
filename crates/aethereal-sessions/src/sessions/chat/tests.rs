use super::control_response;

#[test]
fn an_answer_is_written_as_the_control_response_claude_code_reads_for_its_request() {
    // Tripwire: the line written to Claude Code's stdin for its `can_use_tool` request, allowed and denied. A response
    // that names another request, nests its answer elsewhere, or allows without the input it asked to use is one
    // Claude Code does not take, and its turn waits for good.
    let allowed = control_response("ask-1", true, r#"{"command":"ls"}"#);
    let denied = control_response("ask-2", false, r#"{"command":"rm -rf /"}"#);

    assert_eq!(
        allowed,
        concat!(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"ask-1","#,
            r#""response":{"behavior":"allow","updatedInput":{"command":"ls"}}}}"#,
            "\n"
        )
    );
    assert_eq!(
        denied,
        concat!(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"ask-2","#,
            r#""response":{"behavior":"deny","message":"The owner did not allow it."}}}"#,
            "\n"
        )
    );
}
