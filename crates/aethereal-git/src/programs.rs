//! What each program the root runs is started with (ADR-0040): as it begins, `git init --bare` and the check that the
//! hook is in place; for each request, `git http-backend`. Each states its environment whole, so nothing of the host's
//! reaches it, and of a request only what is named here.

use crate::{Call, GitSettings};
use aethereal_actor::ProtocolRef;
use aethereal_http::stack::Request;
use aethereal_process::{Environment, Exits, Start, Stdio, Variable};

/// The `PATH` every program is given: where a git named by its name alone is found, and the programs its hook runs.
/// git finds its own programs, `git-upload-pack` and the rest, where it was built to.
const PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// The shell the hook's check runs in, which the hook names as its own interpreter.
const SHELL: &str = "/bin/sh";

/// The hook every push runs, in the directory of hooks.
const HOOK: &str = "pre-receive";

/// `git init --bare` of the repository, its first branch `main`, which leaves a repository already there as it is.
pub fn init(settings: &GitSettings, starter: ProtocolRef<Exits>) -> Start {
    let args = ["init", "--bare", "--quiet", "--initial-branch=main", "--", settings.repository.as_str()];

    Start {
        program: settings.program.clone(),
        args: args.map(String::from).to_vec(),
        environment: Environment::Only(configured(settings)),
        stdin: Stdio::Closed,
        stdout: Stdio::Closed,
        starter,
    }
}

/// The check that the hook is in place: a file git can run, in the directory of hooks. Without it git runs no hook,
/// and every push would be taken, so the repository is not served until it is.
pub fn check(settings: &GitSettings, starter: ProtocolRef<Exits>) -> Start {
    let hook = format!("{}/{HOOK}", settings.hooks);
    let args = ["-c", "test -f \"$1\" && test -x \"$1\"", "sh", hook.as_str()];

    Start {
        program: String::from(SHELL),
        args: args.map(String::from).to_vec(),
        environment: Environment::Only(Vec::new()),
        stdin: Stdio::Closed,
        stdout: Stdio::Closed,
        starter,
    }
}

/// `git http-backend` for `call`, which `request` made for the session numbered `session`: its stdin the request's
/// body, and its stdout the response, its CGI head and then its body.
pub fn backend<A, C>(
    settings: &GitSettings,
    call: Call,
    session: u32,
    request: &Request<A, C>,
    starter: ProtocolRef<Exits>,
) -> Start {
    // CGI's variables: what was asked, named by the call itself, and who asked. `REMOTE_USER` is the session the
    // hook lets push its own branches, and `git http-backend` takes a push only from a user it names.
    let user = format!("session-{session}");
    let asked = [
        ("REQUEST_METHOD", call.method()),
        ("PATH_INFO", call.path_info()),
        ("QUERY_STRING", call.query()),
        ("REMOTE_USER", user.as_str()),
        // Every repository beneath the root is served, and the root is the repository itself.
        ("GIT_PROJECT_ROOT", settings.repository.as_str()),
        ("GIT_HTTP_EXPORT_ALL", "1"),
    ];
    // What git's client says of its body and its protocol: the body's type, which the program checks; its coding, as a
    // large fetch's request comes gzipped; and the protocol version it asks for, without which it is spoken version 0.
    let said = [
        ("CONTENT_TYPE", request.header("content-type")),
        ("HTTP_CONTENT_ENCODING", request.header("content-encoding")),
        ("GIT_PROTOCOL", request.header("git-protocol")),
    ];
    let said = said.into_iter().filter_map(|(name, value)| Some((name, value?)));
    let mut environment = configured(settings);
    environment.extend(asked.into_iter().chain(said).map(|(name, value)| variable(name, value)));

    Start {
        program: settings.program.clone(),
        args: vec![String::from("http-backend")],
        environment: Environment::Only(environment),
        stdin: Stdio::Connected,
        stdout: Stdio::Connected,
        starter,
    }
}

/// What every git the root runs is configured by: the `PATH` its hook runs on; no configuration of the machine's or of
/// a user's, so nothing outside the repository and these settings changes what it does; and the hooks the settings
/// name, given as configuration on its command line's level, above the repository's own, so nothing in the repository
/// chooses them.
fn configured(settings: &GitSettings) -> Vec<Variable> {
    let set = [
        ("PATH", PATH),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_0", settings.hooks.as_str()),
    ];

    set.into_iter().map(|(name, value)| variable(name, value)).collect()
}

/// The variable `name`, set to `value`.
fn variable(name: &str, value: &str) -> Variable {
    Variable { name: String::from(name), value: String::from(value) }
}
