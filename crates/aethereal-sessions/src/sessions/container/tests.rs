use super::{Made, owner, spec};
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
    // print mode's own model, or as a new conversation; a home volume named apart from its run's is a new home each
    // run, where `--resume` finds no conversation.
    let conversation = Conversation::Begun(String::from("0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b"));
    let env = vec![Variable { name: String::from("TOKEN"), value: String::from("t") }];
    let made = Made { session: 7, run: 2, conversation: &conversation, env };
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
        r#"{"Type":"bind","Source":"/opt/claude/claude","Target":"/usr/local/bin/claude","ReadOnly":true},"#,
        r#"{"Type":"volume","Source":"aethereal-session-hub-7","Target":"/session/home","ReadOnly":false,"#,
        r#""VolumeOptions":{"NoCopy":true}}]}}"#,
    );
    assert_eq!(body.as_deref(), Ok(expected));
    assert_eq!(spec.name, "aethereal-session-hub-7-2");
}

#[test]
fn the_container_that_gives_a_home_to_its_user_is_root_alone_with_the_home_and_nothing_else() {
    // Tripwire: the create body of the container that gives session 7's home to its user. Run as the session's user
    // it cannot change the owner, so every later container finds its home root's; given the network, the credential,
    // or a bind of this machine's, a step that runs as root holds more than the one directory it changes.
    let Ok(spec) = owner(&settings(), 7, 1) else {
        panic!("the settings' labels are labels, and its label names a volume");
    };
    let body = spec.body();
    let expected = concat!(
        r#"{"Image":"rust:1.97-slim","User":"0:0","WorkingDir":"/","Cmd":["chown","1000:1000","/session/home"],"#,
        r#""Env":[],"Labels":{"aethereal.sessions":"hub","aethereal.test":"1","aethereal.session":"7"},"#,
        r#""OpenStdin":false,"StdinOnce":false,"AttachStdin":false,"AttachStdout":true,"AttachStderr":true,"#,
        r#""Tty":false,"HostConfig":{"NetworkMode":"none","NanoCpus":4000000000,"Memory":8589934592,"Mounts":["#,
        r#"{"Type":"volume","Source":"aethereal-session-hub-7","Target":"/session/home","ReadOnly":false,"#,
        r#""VolumeOptions":{"NoCopy":true}}]}}"#,
    );
    assert_eq!(body.as_deref(), Ok(expected));
    assert_eq!(spec.name, "aethereal-session-hub-7-1-home");
}
