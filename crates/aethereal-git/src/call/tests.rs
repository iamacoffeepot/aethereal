use super::{Call, Service};
use aethereal_http::{Method, Status};

#[test]
fn git_s_three_requests_are_read_as_their_calls_wherever_their_route_stands() {
    // Catches a push read as a fetch or the other way, which would run the wrong service, and a call read from where
    // one hub's routes put git rather than from the path's last segments.
    let refs = Call::of(Method::Get, "/git/info/refs", "service=git-receive-pack");
    let fetch = Call::of(Method::Post, "/git/git-upload-pack", "");
    let push = Call::of(Method::Post, "/elsewhere/git-receive-pack", "");

    assert_eq!(refs, Ok(Call::Refs(Service::ReceivePack)));
    assert_eq!(fetch, Ok(Call::Exchange(Service::UploadPack)));
    assert_eq!(push, Ok(Call::Exchange(Service::ReceivePack)));
}

#[test]
fn refs_asked_with_no_service_or_another_are_forbidden_and_a_path_that_is_none_of_git_s_is_not_found() {
    // Catches git's dumb protocol served, which reads the repository's files as they lie, past every service's rules,
    // and a query handed on that names a service the program would run.
    let dumb = Call::of(Method::Get, "/git/info/refs", "");
    let other = Call::of(Method::Get, "/git/info/refs", "service=git-upload-archive");
    let padded = Call::of(Method::Get, "/git/info/refs", "service=git-upload-pack&x=1");
    let elsewhere = Call::of(Method::Post, "/git/objects/info/packs", "");
    let deleted = Call::of(Method::Delete, "/git/git-receive-pack", "");

    assert_eq!(dumb, Err(Status::Forbidden));
    assert_eq!(other, Err(Status::Forbidden));
    assert_eq!(padded, Err(Status::Forbidden));
    assert_eq!(elsewhere, Err(Status::NotFound));
    assert_eq!(deleted, Err(Status::NotFound));
}
