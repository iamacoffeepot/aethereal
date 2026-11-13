use super::{acted, created, inspected, listed};
use crate::{Acted, ContainerId, ContainerState, Created, DockerError, Inspected, Label, Listed, Listing, Phase};

/// An id as Docker writes one.
const ID: &str = "4f9a0c1e2b3d4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f90";

fn id() -> ContainerId {
    ContainerId::read(ID).unwrap_or_else(|| panic!("the id is 64 hex digits"))
}

#[test]
fn error_replies_are_named_by_their_status_with_the_message_docker_gave() {
    // Catches a 404 or a 409 read as some other failure, so a session cannot tell a container gone from one in the
    // wrong state; Docker's message lost; and a body that is not Docker's JSON lost rather than kept as its text.
    let missing = br#"{"message":"No such container: aethereal-7"}"#;
    assert_eq!(
        acted(404, missing),
        Acted::Failed(DockerError::NotFound { message: String::from("No such container: aethereal-7") })
    );
    let taken = br#"{"message":"Conflict. The container name \"/aethereal-7\" is already in use"}"#;
    assert_eq!(
        created(409, taken),
        Created::Failed(DockerError::Conflict {
            message: String::from("Conflict. The container name \"/aethereal-7\" is already in use"),
        })
    );
    assert_eq!(
        inspected(500, b"daemon is shutting down\n"),
        Inspected::Failed(DockerError::Refused { status: 500, message: String::from("daemon is shutting down") })
    );
    assert_eq!(acted(304, b""), Acted::Unchanged, "a container started or stopped already");
}

#[test]
fn a_create_is_answered_with_the_id_docker_gave_and_nothing_else_passes_for_one() {
    // Catches an id read from the wrong member, and one that is not Docker's taken as one, which a later request would
    // write into its path.
    let made = format!(r#"{{"Id":"{ID}","Warnings":[]}}"#);
    assert_eq!(created(201, made.as_bytes()), Created::Made(id()));
    assert_eq!(created(201, br#"{"Id":"../../images/x","Warnings":[]}"#), Created::Failed(DockerError::Malformed));
}

#[test]
fn an_inspect_is_read_as_the_state_its_container_is_in() {
    // Catches the exit code or the kernel's OOM kill read from the wrong member, or kept for a container still
    // running; and a state missing what it is read by taken as some default.
    let running = br#"{"Id":"x","State":{"Status":"running","Running":true,"OOMKilled":false,"ExitCode":0,"Pid":71}}"#;
    assert_eq!(inspected(200, running), Inspected::State(ContainerState::Running));
    let killed = br#"{"State":{"Status":"exited","Running":false,"OOMKilled":true,"ExitCode":137},"ExitCode":3}"#;
    assert_eq!(inspected(200, killed), Inspected::State(ContainerState::Exited { code: 137, oom_killed: true }));
    let partial = br#"{"State":{"Status":"exited","Running":false}}"#;
    assert_eq!(inspected(200, partial), Inspected::Failed(DockerError::Malformed));
}

#[test]
fn a_list_is_read_as_each_container_its_name_phase_and_labels() {
    // Catches the `/` Docker writes before a name kept, a container's labels lost or crossed with another's, and a
    // list with a container it cannot read answered short rather than as malformed.
    let list = format!(
        r#"[{{"Id":"{ID}","Names":["/aethereal-7"],"State":"exited","Status":"Exited (0) 2 minutes ago",
            "Labels":{{"aethereal.session":"7","aethereal.test":"1"}}}},
           {{"Id":"{ID}","Names":["/aethereal-8"],"State":"running","Labels":{{}}}}]"#
    );
    let session = |name: &str, phase, labels| Listing { id: id(), name: String::from(name), phase, labels };
    let labelled = vec![
        Label { key: String::from("aethereal.session"), value: String::from("7") },
        Label { key: String::from("aethereal.test"), value: String::from("1") },
    ];
    assert_eq!(
        listed(200, list.as_bytes()),
        Listed::Containers(vec![
            session("aethereal-7", Phase::Exited, labelled),
            session("aethereal-8", Phase::Running, Vec::new()),
        ])
    );
    let unreadable = format!(r#"[{{"Id":"{ID}","Names":["/a"],"State":"running","Labels":{{}}}},{{"Id":"short"}}]"#);
    assert_eq!(listed(200, unreadable.as_bytes()), Listed::Failed(DockerError::Malformed));
}
