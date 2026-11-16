use super::{Made, spec};
use crate::Variable;
use crate::sessions::record::Conversation;
use crate::sessions::settings::SessionsSettings;

fn settings() -> SessionsSettings {
    SessionsSettings {
        claude: String::from("/opt/claude/claude"),
        model: String::from("opus"),
        label: String::from("hub"),
        labels: vec![String::from("aethereal.test=1")],
        hub_url: String::from("http://127.0.0.1:8890/mcp"),
        ..SessionsSettings::default()
    }
}

#[test]
fn a_session_s_container_is_written_from_the_settings_and_the_session_alone() {
    // Tripwire: the create body of session 7's second run, which resumes Claude's conversation. A limit, a mount, the
    // model, the label a list finds it by, or the resume missing from it runs the session unbounded, unfound, on
    // print mode's own model, or as a new conversation.
    let conversation = Conversation::Begun(String::from("0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b"));
    let env = vec![Variable { name: String::from("TOKEN"), value: String::from("t") }];
    let made = Made { session: 7, run: 2, conversation: &conversation, home: String::from("/s/7/home"), env };
    let Ok(spec) = spec(&settings(), made) else {
        panic!("the settings' labels are labels");
    };
    let body = spec.body();
    let expected = concat!(
        r#"{"Image":"rust:1.97-slim","User":"1000:1000","WorkingDir":"/session/home","Cmd":["/usr/local/bin/claude","#,
        r#""-p","--input-format","stream-json","--output-format","stream-json","--verbose","--model","opus","#,
        r#""--dangerously-skip-permissions","--resume","0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b","--mcp-config","#,
        r#""{\"mcpServers\":{\"aethereal\":{\"type\":\"http\",\"url\":\"http://127.0.0.1:8890/mcp\"}}}"],"#,
        r#""Env":["HOME=/session/home","TOKEN=t"],"#,
        r#""Labels":{"aethereal.sessions":"hub","aethereal.test":"1","aethereal.session":"7"},"#,
        r#""OpenStdin":true,"StdinOnce":false,"AttachStdin":true,"AttachStdout":true,"AttachStderr":true,"Tty":false,"#,
        r#""HostConfig":{"NetworkMode":"host","NanoCpus":4000000000,"Memory":8589934592,"Mounts":["#,
        r#"{"Type":"bind","Source":"/s/7/home","Target":"/session/home","ReadOnly":false},"#,
        r#"{"Type":"bind","Source":"/opt/claude/claude","Target":"/usr/local/bin/claude","ReadOnly":true}]}}"#,
    );
    assert_eq!(body.as_deref(), Ok(expected));
    assert_eq!(spec.name, "aethereal-session-hub-7-2");
}
