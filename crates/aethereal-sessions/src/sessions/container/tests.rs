use super::{Made, spec, volumes};
use crate::Bearer;
use crate::sessions::record::Conversation;
use crate::sessions::settings::SessionsSettings;
use crate::{Label, Variable};

fn settings() -> SessionsSettings {
    SessionsSettings {
        model: String::from("opus"),
        prompt: String::from("You run in a container."),
        label: String::from("hub"),
        labels: vec![String::from("aethereal.test=1")],
        hub_url: String::from("http://172.17.0.1:8893/mcp"),
        git_url: String::from("http://172.17.0.1:8893/git"),
        bootstrap_secs: 300,
        ..SessionsSettings::default()
    }
}

#[test]
fn a_session_s_container_is_written_from_the_settings_and_the_session_alone() {
    // Tripwire: the create body of session 7's second run, which resumes Claude's conversation. It binds nothing of
    // the machine but its drop box, as Claude Code is the image's. A limit, a mount, the
    // model, the prompt, the label a list finds it by, or the resume missing from it runs the session unbounded,
    // unfound, on print mode's own model, untold where it is, or as a new conversation; a home or target volume named
    // apart from its session's is a new one each run, where `--resume` finds no conversation and cargo builds the
    // world again; and a cargo home named for the session is a registry fetched again by every session. The bearer
    // reaches its MCP config and git only through the environment, never the command, and the container is on a
    // network of its own, where the owner's port on the loopback is not. The command is the start script's, which runs
    // Claude Code once the work tree is ready; without the session's number, the repository's URL, or the steps'
    // limit, the script makes no git author, clones nothing, or fails the bootstrap; and the hub's own variables come
    // after the credential's, so a line of its file never stands in for one.
    let conversation = Conversation::Begun(String::from("0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b"));
    let env = vec![Variable { name: String::from("TOKEN"), value: String::from("t") }];
    let bearer = Bearer([0xab; 16]);
    let drop_box = String::from("/srv/store/drops/7");
    let made = Made { session: 7, run: 2, conversation: &conversation, env, bearer, drop_box };
    let Ok(spec) = spec(&settings(), made) else {
        panic!("the settings' labels are labels");
    };
    let body = spec.body();
    let expected = [
        r#"{"Image":"aethereal-session","User":"1000:1000","WorkingDir":"/session/home","#,
        r#""Cmd":["/usr/local/bin/aethereal-session-start","/usr/bin/claude","-p","#,
        r#""--input-format","stream-json","--output-format","stream-json","#,
        r#""--verbose","--model","opus","--dangerously-skip-permissions","#,
        r#""--append-system-prompt","You run in a container.","#,
        r#""--resume","0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b","--mcp-config","#,
        r#""{\"mcpServers\":{\"aethereal\":{\"type\":\"http\",\"url\":\"http://172.17.0.1:8893/mcp\","#,
        r#"\"headers\":{\"Authorization\":\"Bearer ${AETHEREAL_HUB_BEARER}\"}}}}"],"#,
        r#""Env":["HOME=/session/home","CARGO_HOME=/session/cargo","CARGO_TARGET_DIR=/session/target","TOKEN=t","#,
        r#""AETHEREAL_SESSION=7","AETHEREAL_GIT_URL=http://172.17.0.1:8893/git","AETHEREAL_BOOTSTRAP_SECS=300","#,
        &format!(r#""AETHEREAL_HUB_BEARER={}"],"#, bearer.hex()),
        r#""Labels":{"aethereal.sessions":"hub","aethereal.test":"1","aethereal.session":"7"},"#,
        r#""OpenStdin":true,"StdinOnce":false,"AttachStdin":true,"AttachStdout":true,"AttachStderr":true,"Tty":false,"#,
        r#""HostConfig":{"NetworkMode":"bridge","NanoCpus":4000000000,"Memory":8589934592,"Mounts":["#,
        r#"{"Type":"bind","Source":"/srv/store/drops/7","Target":"/session/drop","ReadOnly":false},"#,
        r#"{"Type":"volume","Source":"aethereal-session-hub-7","Target":"/session/home","ReadOnly":false},"#,
        r#"{"Type":"volume","Source":"aethereal-session-hub-7-target","Target":"/session/target","ReadOnly":false},"#,
        r#"{"Type":"volume","Source":"aethereal-cargo-hub","Target":"/session/cargo","ReadOnly":false}]}}"#,
    ]
    .concat();
    assert_eq!(body.as_deref(), Ok(expected.as_str()));
    assert_eq!(spec.name, "aethereal-session-hub-7-2");
}

#[test]
fn a_session_s_own_volumes_carry_its_number_and_the_shared_cargo_home_the_hub_s_labels_alone() {
    // Catches the shared cargo home labelled for the session that made it first, so it reads as that session's and
    // is taken with it, and a session's own volume made without its number, which nothing finds it by.
    let Ok(volumes) = volumes(&settings(), 7) else {
        panic!("the settings' labels are labels, and its label names a volume");
    };
    let hub = [("aethereal.sessions", "hub"), ("aethereal.test", "1")];
    let session = [("aethereal.sessions", "hub"), ("aethereal.test", "1"), ("aethereal.session", "7")];
    let labelled = |pairs: &[(&str, &str)]| -> Vec<Label> {
        pairs.iter().map(|(key, value)| Label { key: String::from(*key), value: String::from(*value) }).collect()
    };
    let made: Vec<(&str, &[Label])> =
        volumes.iter().map(|volume| (volume.name.as_str(), volume.labels.as_slice())).collect();

    let own = labelled(&session);
    let shared = labelled(&hub);
    let expected: [(&str, &[Label]); 3] =
        [("aethereal-session-hub-7", &own), ("aethereal-session-hub-7-target", &own), ("aethereal-cargo-hub", &shared)];
    assert_eq!(made, expected);
}
