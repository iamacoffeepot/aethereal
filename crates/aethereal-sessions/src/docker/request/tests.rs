use crate::{ContainerId, Events, Label, List, Logs, Until, Unwritable};

#[test]
fn a_list_and_the_events_are_filtered_by_their_label_and_every_filter_is_percent_encoded() {
    // Tripwire: the queries Docker reads a list's and the events' filters from, as JSON percent-encoded. A filter
    // written raw breaks the query at its `&` or `=`, and the events would follow every container on the daemon.
    let label = Label { key: String::from("aethereal.session"), value: String::from("a b&c") };
    let listed = List { label: label.clone() }.call().map(|call| (call.path, call.query));
    assert_eq!(
        listed,
        Ok((
            String::from("/v1.44/containers/json"),
            String::from("all=1&filters=%7B%22label%22%3A%5B%22aethereal.session%3Da%20b%26c%22%5D%7D")
        ))
    );
    let watched = Events { label }.call().map(|call| call.query);
    assert_eq!(
        watched,
        Ok(String::from(
            "filters=%7B%22type%22%3A%5B%22container%22%5D%2C%22event%22%3A%5B%22start%22%2C%22die%22%2C%22oom%22%5D%2C%22label%22%3A%5B%22aethereal.session%3Da%20b%26c%22%5D%7D"
        ))
    );

    let ambiguous = Label { key: String::from("a=b"), value: String::from("c") };
    assert_eq!(List { label: ambiguous }.call(), Err(Unwritable::Label { key: String::from("a=b") }));
}

#[test]
fn logs_are_asked_from_their_time_to_the_nanosecond() {
    // Catches `since` written in whole seconds, so a reader catching up reads again what it already has, or its
    // nanoseconds written without their leading zeros, a time later than asked.
    let container = ContainerId::read("4f9a0c1e2b3d4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f90");
    let Some(container) = container else {
        panic!("the id is 64 hex digits");
    };
    let logs = Logs { container, since_nanos: 1_763_020_800_000_000_042, until: Until::Stopped };
    let query = logs.call().map(|call| call.query);
    assert_eq!(query, Ok(String::from("stdout=1&stderr=1&since=1763020800.000000042&follow=1")));
}
