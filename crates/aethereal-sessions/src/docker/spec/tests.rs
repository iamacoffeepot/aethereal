use crate::{Access, Bind, ContainerSpec, Create, Label, Network, Stdin, Unwritable, Variable};

/// A session's container, as a capability's config would state one.
fn spec() -> ContainerSpec {
    ContainerSpec {
        name: String::from("aethereal-session-7"),
        image: String::from("aethereal-session:1"),
        user: String::from("1000:1000"),
        working_dir: String::from("/work/project"),
        command: vec![String::from("claude"), String::from("-p"), String::from("--model \"x\"")],
        env: vec![Variable { name: String::from("HOME"), value: String::from("/work/home") }],
        binds: vec![
            Bind {
                source: String::from("/srv/s/7/project"),
                target: String::from("/work/project"),
                access: Access::ReadWrite,
            },
            Bind {
                source: String::from("/srv/bin/claude"),
                target: String::from("/usr/bin/claude"),
                access: Access::ReadOnly,
            },
        ],
        network: Network::Named(String::from("aethereal")),
        millicpus: 4000,
        memory_bytes: 8 << 30,
        labels: vec![Label { key: String::from("aethereal.session"), value: String::from("7") }],
        stdin: Stdin::Kept,
    }
}

#[test]
fn a_spec_is_written_as_the_create_body_docker_documents() {
    // Tripwire: the body of `POST /containers/create` a spec writes. A member misnamed or misplaced is silently
    // ignored by Docker, so the container runs without the limit, the mount, or the open stdin the spec asked for;
    // `Tty` must stay false, or the output is not framed and nothing demultiplexes it.
    let body = spec().body();
    let expected = concat!(
        r#"{"Image":"aethereal-session:1","User":"1000:1000","WorkingDir":"/work/project","#,
        r#""Cmd":["claude","-p","--model \"x\""],"Env":["HOME=/work/home"],"Labels":{"aethereal.session":"7"},"#,
        r#""OpenStdin":true,"StdinOnce":false,"AttachStdin":true,"AttachStdout":true,"AttachStderr":true,"Tty":false,"#,
        r#""HostConfig":{"NetworkMode":"aethereal","NanoCpus":4000000000,"Memory":8589934592,"Mounts":["#,
        r#"{"Type":"bind","Source":"/srv/s/7/project","Target":"/work/project","ReadOnly":false},"#,
        r#"{"Type":"bind","Source":"/srv/bin/claude","Target":"/usr/bin/claude","ReadOnly":true}]}}"#,
    );
    assert_eq!(body.as_deref(), Ok(expected));

    let query = Create { spec: spec() }.call().map(|call| (call.path, call.query));
    assert_eq!(query, Ok((String::from("/v1.44/containers/create"), String::from("name=aethereal-session-7"))));
}

#[test]
fn a_variable_docker_would_split_elsewhere_is_refused_before_anything_is_sent() {
    // Catches a name holding `=` written as is, so the container sees another variable than the spec states.
    let mut split = spec();
    split.env.push(Variable { name: String::from("A=B"), value: String::from("C") });
    assert_eq!(split.body(), Err(Unwritable::Variable { name: String::from("A=B") }));
}
