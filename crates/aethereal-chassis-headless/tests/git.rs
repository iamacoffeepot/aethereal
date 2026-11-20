//! The git capability on the native host (ADR-0057, ADR-0035, ADR-0040): the repository made as the root begins, and
//! served through a stack of the test's own, on the hub's git routes, to git's own client, run as a program of this
//! machine on a thread of its own, with a stand-in for the sessions' bearers; and the session image's start script,
//! run as a container runs it, readying a session's work tree from it. Each test says it is skipped, and passes, where
//! this machine has no `git`.
#![cfg(unix)]

use aethereal_actor::path;
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::Plain;
use aethereal_git::{Git, GitSettings, Readiness};
use aethereal_http::stack::{Credentials, Head, Layer, LayerForm, Response, StackSettings, Step};
use aethereal_http::{Method, Route, Router, Routes, Unroutable};
use aethereal_process::Processes;
use aethereal_sessions::BOOTSTRAP_FAILED;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::{Interface, Tcp};
use aethereal_tools::Caller;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs, process, thread};

mod git_client;
mod stacks;

use git_client::{git, has_git};
use stacks::{bound, stack};

/// A stand-in for the sessions' actor layer: a pure layer that reads the password of `Basic` credentials, `<n>`, as
/// session `n`, and answers any other request with the sessions' challenge, `Basic` in the realm `aethereal`, which git's client answers with the credentials
/// its remote's URL holds. A test then pushes as a session without minting its bearer.
#[derive(Debug, Default, Clone, Plain)]
struct Badge;

impl LayerForm for Badge {
    type Form = Self;
}

impl Layer for Badge {
    type In = ();
    type Out = Caller;

    fn inbound(&mut self, head: &Head, (): ()) -> Step<Caller> {
        let session = match &head.credentials {
            Credentials::Basic { password, .. } => password.parse().ok(),
            Credentials::None | Credentials::Bearer { .. } | Credentials::Other { .. } => None,
        };

        session.map_or_else(
            || Step::Answer(Response::challenge("Basic", "aethereal")),
            |session| Step::Pass(Caller::Session { session }),
        )
    }
}

/// git's routes, as the hub's sessions' port serves them.
struct GitRoutes;

impl LayerForm for GitRoutes {
    type Form = Router<Self>;
}

impl Routes for GitRoutes {
    type Attached = Caller;
    type Handlers = (Route<Git, ()>,);

    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
        router
            .route(Method::Get, "/git/info/refs", |()| path::<Git>(()))?
            .route(Method::Post, "/git/git-upload-pack", |()| path::<Git>(()))?
            .route(Method::Post, "/git/git-receive-pack", |()| path::<Git>(()))
    }
}

stack!(GitStack, "test.git.git_stack", "git_stack", (Badge, GitRoutes), [], depends(Git));

/// The session the tests push as.
const SESSION: u32 = 3;

/// A directory of the test's own, named for it, removed when dropped: the repository, the clones, and git's home.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Self {
        let dir = env::temp_dir().join(format!("aethereal-git-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");

        Self { dir }
    }

    /// The path `name` in it.
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The settings of a root serving the repository `repository.git` in it, which is not there yet, with the crate's
    /// own hooks.
    fn settings(&self) -> GitSettings {
        GitSettings { repository: self.path("repository.git").display().to_string(), ..GitSettings::default() }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// What `command` came to, run to its end, reaching nothing the host serves.
fn ran(mut command: Command) -> Output {
    let Ok(output) = command.output() else {
        panic!("git runs: {command:?}");
    };

    output
}

/// What `command` came to, run to its end on a thread of its own, outside the engine, as the host `harness` is stepped:
/// it reaches the stack the host serves.
fn served(harness: &mut Harness<Headless>, mut command: Command) -> Output {
    #[allow(clippy::disallowed_methods)] // git's client runs on a native thread of its own, outside the engine.
    let running = thread::spawn(move || command.output());
    waiting::until(BUDGET, || {
        harness.step();

        running.is_finished()
    });
    assert!(running.is_finished(), "git's client finishes within the wait");
    let Ok(Ok(output)) = running.join() else {
        panic!("git's client runs");
    };

    output
}

/// What git wrote to its stderr, which says why it failed.
fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A stack's settings as a test serves it: any port the host picks, on the loopback.
fn on_loopback() -> StackSettings {
    StackSettings { port: 0, interface: Interface::Loopback, ..StackSettings::default() }
}

/// A host serving the repository `git` names through a stack of `stack`'s settings, once the repository is made or
/// could not be: the host, and the stack's port.
fn hub(git: GitSettings, stack: StackSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let read_limit = stack.most_read_bytes().try_into().unwrap_or(u32::MAX);
    let engine = EngineSettings { read_limit, ..EngineSettings::default() };
    let program = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root::<Processes>()
        .root_with::<Git>(git)
        .root_with::<GitStack>(stack);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &GitStack| stack.serving.port());

    let root = harness.root::<Git>();
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(root).is_ok_and(|git| git.readiness() != Readiness::Making)
    });

    Ok((harness, port))
}

/// Where the repository the host serves stands.
fn readiness(harness: &mut Harness<Headless>) -> Result<Readiness, Unheard> {
    let root = harness.root::<Git>();

    Ok(harness.state(root)?.readiness())
}

/// How many requests the host has started a program for.
fn runs(harness: &mut Harness<Headless>) -> Result<u64, Unheard> {
    let root = harness.root::<Git>();

    Ok(harness.state(root)?.runs())
}

/// The repository's URL on `port` as session `session` reaches it: its bearer, `<session>`, is the password git's
/// client answers a challenge with.
fn remote(port: u16, session: u32) -> String {
    format!("http://session:{session}@127.0.0.1:{port}/git")
}

/// The clone `name` of the repository the host serves on `port`, made in the test's directory as session `session`.
fn cloned(harness: &mut Harness<Headless>, port: u16, scratch: &Scratch, name: &str) -> PathBuf {
    let cloning = git(&scratch.dir, &["clone", "--quiet", &remote(port, SESSION), name]);
    let clone = served(harness, cloning);
    assert!(clone.status.success(), "the session clones the repository: {}", said(&clone));

    scratch.path(name)
}

/// The commit `reference` names in the repository at `repository`: none when it names nothing.
fn named(repository: &Path, reference: &str) -> Option<String> {
    let output = ran(git(repository, &["rev-parse", "--verify", "--quiet", reference]));

    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Commits the file `file`, holding `bytes`, in the clone at `work`, and answers the commit.
fn commit(work: &Path, file: &str, bytes: &[u8]) -> Option<String> {
    assert!(fs::write(work.join(file), bytes).is_ok(), "the test writes {file}");
    let added = ran(git(work, &["add", file]));
    let committed = ran(git(work, &["commit", "--quiet", "-m", file]));
    assert!(added.status.success(), "{}", said(&added));
    assert!(committed.status.success(), "{}", said(&committed));

    named(work, "HEAD")
}

/// `length` bytes no compression shrinks, the same on every run: a xorshift's.
fn noise(length: usize) -> Vec<u8> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.to_le_bytes()[0]
    };

    (0..length).map(|_| next()).collect()
}

#[test]
fn the_repository_is_made_where_none_was_and_a_session_clones_it() -> Result<(), Unheard> {
    // Catches a repository not made as the root begins, made where the settings do not name, or made with a work tree,
    // which `git http-backend` serves none of; and refs a session asks for whose CGI head is not read, or whose body is
    // not streamed to the client whole.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("made");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;

    let bare = ran(git(&scratch.path("repository.git"), &["rev-parse", "--is-bare-repository"]));
    let work = cloned(&mut harness, port, &scratch, "work");

    assert_eq!(readiness(&mut harness)?, Readiness::Ready);
    assert_eq!(String::from_utf8_lossy(&bare.stdout).trim(), "true", "{}", said(&bare));
    assert!(work.join(".git").is_dir(), "the clone is made");

    Ok(())
}

#[test]
fn a_session_creates_force_moves_and_deletes_its_own_branches() -> Result<(), Unheard> {
    // Catches a request's body not written to its program's stdin whole, or the stdin never finished, so a push hangs
    // or its pack does not check; a session's own branch refused, or a forced move or a deletion of it; and a push the
    // client is told was taken that the repository does not hold.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("own");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    let repository = scratch.path("repository.git");
    let work = cloned(&mut harness, port, &scratch, "work");
    let branch = format!("refs/heads/sessions/{SESSION}/x");
    let target = format!("HEAD:{branch}");

    let first = commit(&work, "first", b"first");
    let created = served(&mut harness, git(&work, &["push", "--quiet", "origin", &target]));

    assert!(created.status.success(), "{}", said(&created));
    assert_eq!(named(&repository, &branch), first);

    let amended = ran(git(&work, &["commit", "--quiet", "--amend", "-m", "moved"]));
    let moved = named(&work, "HEAD");
    let forced = served(&mut harness, git(&work, &["push", "--quiet", "--force", "origin", &target]));

    assert!(amended.status.success(), "{}", said(&amended));
    assert_ne!(moved, first, "the amend makes a commit that does not follow the first");
    assert!(forced.status.success(), "{}", said(&forced));
    assert_eq!(named(&repository, &branch), moved);

    let deleted = served(&mut harness, git(&work, &["push", "--quiet", "origin", &format!(":{branch}")]));

    assert!(deleted.status.success(), "{}", said(&deleted));
    assert_eq!(named(&repository, &branch), None);

    Ok(())
}

#[test]
fn a_session_is_refused_main_a_tag_and_another_session_s_branch_each_said_why() -> Result<(), Unheard> {
    // Catches the hook not run, as git finds none where the settings name it, or one the repository chose run in its
    // place; a ref outside the session's own taken, a session named by another's number read as its own, as
    // `sessions/31/` would be for `sessions/3`; and a refusal its pusher is not told the reason of.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("refused");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    let repository = scratch.path("repository.git");
    let work = cloned(&mut harness, port, &scratch, "work");
    commit(&work, "first", b"first");

    let refs = ["refs/heads/main", "refs/tags/v1", "refs/heads/sessions/4/x", "refs/heads/sessions/31/x"];
    for refused in refs {
        let pushed = served(&mut harness, git(&work, &["push", "origin", &format!("HEAD:{refused}")]));
        let why = format!("refused {refused}: session {SESSION} pushes only branches under sessions/{SESSION}/");

        assert!(!pushed.status.success(), "a push of {refused} is refused");
        assert!(said(&pushed).contains(&why), "the pusher is told why: {}", said(&pushed));
        assert_eq!(named(&repository, refused), None);
    }

    Ok(())
}

#[test]
fn a_request_bearing_no_session_is_challenged_and_starts_no_program() -> Result<(), Unheard> {
    // Catches git's routes served past the layer that says who sent a request, so a program runs, and a push could be
    // taken, for no one; and a challenge git's client does not answer, so a session's own requests never reach git.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("unborne");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    let unborne = format!("http://127.0.0.1:{port}/git");

    let refused = served(&mut harness, git(&scratch.dir, &["ls-remote", &unborne]));
    let unrun = runs(&mut harness)?;
    let listed = served(&mut harness, git(&scratch.dir, &["ls-remote", &remote(port, SESSION)]));

    assert!(!refused.status.success(), "a request with no session is refused");
    assert!(
        said(&refused).contains("could not read Username"),
        "challenged, git asks for credentials: {}",
        said(&refused)
    );
    assert_eq!(unrun, 0, "no program runs for a request with no session");
    assert!(listed.status.success(), "the session's own request is answered: {}", said(&listed));
    assert!(runs(&mut harness)? > 0, "a program runs for the session's");

    Ok(())
}

#[test]
fn a_pack_larger_than_the_stack_s_window_is_pushed_and_fetched_whole() -> Result<(), Unheard> {
    // Catches a body cut at the stack's window either way, or held whole where it should stream, and a part dropped,
    // sent twice, or sent out of order where the program's pipe or the stack holds it back: the pack would not check,
    // or the file would not come back as it was written.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("large");
    let narrow = StackSettings { chunk_bytes: 4096, window: 2, ..on_loopback() };
    let (mut harness, port) = hub(scratch.settings(), narrow)?;
    let work = cloned(&mut harness, port, &scratch, "work");
    let bytes = noise(2 << 20);
    let branch = format!("sessions/{SESSION}/large");

    commit(&work, "large", &bytes);
    let pushed = served(&mut harness, git(&work, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{branch}")]));
    let fetching = git(&scratch.dir, &["clone", "--quiet", "--branch", &branch, &remote(port, SESSION), "back"]);
    let fetched = served(&mut harness, fetching);

    assert!(pushed.status.success(), "{}", said(&pushed));
    assert!(fetched.status.success(), "{}", said(&fetched));
    let back = fs::read(scratch.path("back").join("large")).unwrap_or_default();
    assert!(back == bytes, "the file comes back as it was written: {} bytes of {}", back.len(), bytes.len());

    Ok(())
}

#[test]
fn a_repository_that_cannot_be_made_or_whose_hook_is_not_in_place_is_never_served() -> Result<(), Unheard> {
    // Catches a repository served whose making failed, which every request would meet as the program's own error; one
    // served with no hook where its settings name it, as git then runs none and takes a session's push of any ref; and
    // a failure of either that stops the program, where the rest of the hub serves on.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("unmade");
    assert!(fs::write(scratch.path("file"), b"").is_ok(), "the test writes a file where a directory would go");
    assert!(fs::create_dir_all(scratch.path("hooks")).is_ok(), "the test makes a directory with no hook");
    let blocked = GitSettings {
        repository: scratch.path("file").join("repository.git").display().to_string(),
        ..scratch.settings()
    };
    let hookless = GitSettings { hooks: scratch.path("hooks").display().to_string(), ..scratch.settings() };

    for unmade in [blocked, hookless] {
        let (mut harness, port) = hub(unmade, on_loopback())?;
        let listed = served(&mut harness, git(&scratch.dir, &["ls-remote", &remote(port, SESSION)]));

        assert_eq!(readiness(&mut harness)?, Readiness::Unmade);
        assert!(!listed.status.success(), "the repository is not served");
        assert!(said(&listed).contains("503"), "{}", said(&listed));
        assert_eq!(runs(&mut harness)?, 0, "no request starts a program");
    }

    Ok(())
}

/// The session image's directory, beside the sessions' crate: the start script, the credential helper, and the system
/// config that names it.
fn image() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../aethereal-sessions/image")
}

/// A session's container, as its start script sees it: the repository at `url`, reached as `bearer`, each step given
/// `secs`.
struct Container<'a> {
    url: &'a str,
    bearer: &'a str,
    secs: u32,
}

impl Container<'_> {
    /// The start script, run as the container runs it, its home `home` in the test's directory, then a fake Claude Code
    /// in the test's directory, given `--verbose`: only the environment the hub and the image give a container, the
    /// image's system config, and its credential helper found by git on the path.
    fn start(&self, scratch: &Scratch, home: &str) -> Command {
        assert!(fs::create_dir_all(scratch.path(home)).is_ok(), "the test makes the session's home, as its volume is");
        let claude = scratch.path("claude");
        let written = fs::write(&claude, CLAUDE);
        assert!(written.is_ok(), "the test writes its fake Claude Code");
        let mut script = Command::new(image().join("aethereal-session-start"));
        script
            .arg("sh")
            .arg(&claude)
            .arg("--verbose")
            .env_clear()
            .env("PATH", searched())
            .env("HOME", scratch.path(home))
            .env("GIT_CONFIG_SYSTEM", image().join("gitconfig"))
            .env("AETHEREAL_SESSION", SESSION.to_string())
            .env("AETHEREAL_GIT_URL", self.url)
            .env("AETHEREAL_HUB_BEARER", self.bearer)
            .env("AETHEREAL_BOOTSTRAP_SECS", self.secs.to_string());

        script
    }
}

/// The fake Claude Code: it prints `claude` to its stdout, marking where its own output begins, and keeps its arguments
/// in its home.
const CLAUDE: &str = "#!/bin/sh\nprintf 'claude\\n'\nprintf '%s\\n' \"$@\" > \"$HOME/claude-ran\"\n";

/// The path a container's programs are found on: the image's directory, where git finds the credential helper, before
/// this machine's own.
fn searched() -> String {
    let machine = env::var("PATH").unwrap_or_default();

    format!("{}:{machine}", image().display())
}

/// What the fake Claude Code was given, once the start script ran it in the home `home`: nothing when it never ran.
fn claude_ran(scratch: &Scratch, home: &str) -> Option<String> {
    fs::read_to_string(scratch.path(home).join("claude-ran")).ok()
}

/// The repository the host serves, given a `main` its owner pushed over the file system, as over ssh, holding the file
/// `first`: the seed it was pushed from.
fn seeded(scratch: &Scratch) -> PathBuf {
    let seed = scratch.path("seed");
    assert!(fs::create_dir_all(&seed).is_ok(), "the test makes the seed");
    let made = ran(git(&seed, &["init", "--quiet", "--initial-branch=main"]));
    assert!(made.status.success(), "{}", said(&made));
    commit(&seed, "first", b"first");
    let repository = scratch.path("repository.git").display().to_string();
    let pushed = ran(git(&seed, &["push", "--quiet", &repository, "HEAD:refs/heads/main"]));
    assert!(pushed.status.success(), "{}", said(&pushed));

    seed
}

#[test]
fn the_start_script_clones_an_empty_home_then_fetches_and_leaves_the_tree_as_it_was_and_runs_claude()
-> Result<(), Unheard> {
    // Catches a tree cloned anywhere but the session's home, or not cloned, as the credential helper never answers the
    // hub's challenge with the bearer; a second start that clones again, resets, checks out, or merges, so an
    // uncommitted edit is lost, or that does not fetch, so a branch pushed meanwhile is not seen; commits that do not
    // say which session made them; Claude Code not run once the tree is ready, or run without its arguments; and the
    // script printing to its stdout, which is Claude Code's, so the hub takes a failed bootstrap for Claude Code's exit.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("start");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    let seed = seeded(&scratch);
    let url = format!("http://127.0.0.1:{port}/git");
    let container = Container { url: &url, bearer: &SESSION.to_string(), secs: 60 };
    let work = scratch.path("home").join("work");

    let first = served(&mut harness, container.start(&scratch, "home"));

    assert!(first.status.success(), "{}", said(&first));
    assert_eq!(first.stdout, b"claude\n", "the clone prints nothing to stdout, before Claude Code does");
    assert_eq!(fs::read(work.join("first")).ok().as_deref(), Some(&b"first"[..]), "main is checked out");
    assert_eq!(claude_ran(&scratch, "home").as_deref(), Some("--verbose\n"));
    let config = scratch.path("home").join(".gitconfig").display().to_string();
    let author = ran(git(&work, &["config", "--file", &config, "user.name"]));
    assert_eq!(String::from_utf8_lossy(&author.stdout).trim(), format!("session {SESSION}"));

    let checked_out = named(&work, "HEAD");
    assert!(fs::write(work.join("first"), b"edited").is_ok(), "the session edits its tree");
    assert!(fs::remove_file(scratch.path("home").join("claude-ran")).is_ok(), "the test forgets the first run");
    let later = commit(&seed, "later", b"later");
    let repository = scratch.path("repository.git").display().to_string();
    let refs = ["HEAD:refs/heads/main", "HEAD:refs/heads/sessions/4/later"];
    let pushed = ran(git(&seed, &[&["push", "--quiet", &repository][..], &refs].concat()));
    assert!(pushed.status.success(), "{}", said(&pushed));
    let second = served(&mut harness, container.start(&scratch, "home"));

    assert!(second.status.success(), "{}", said(&second));
    assert_eq!(second.stdout, b"claude\n", "the fetch prints nothing to stdout, before Claude Code does");
    assert_eq!(named(&work, "refs/remotes/origin/sessions/4/later"), later, "the branch pushed meanwhile is fetched");
    assert_eq!(named(&work, "refs/remotes/origin/main"), later, "main as the owner moved it is fetched");
    assert_eq!(named(&work, "HEAD"), checked_out, "nothing is merged, reset, or checked out");
    assert_eq!(fs::read(work.join("first")).ok().as_deref(), Some(&b"edited"[..]), "the edit is left as it was");
    assert_eq!(claude_ran(&scratch, "home").as_deref(), Some("--verbose\n"));

    Ok(())
}

#[test]
fn a_start_script_clones_past_a_clone_cut_short_and_puts_back_an_origin_that_was_removed() -> Result<(), Unheard> {
    // Catches a clone cut short, as a container killed mid-clone leaves it, left in the way of every clone after it, so
    // the session never bootstraps again, or taken for the tree; and a tree whose `origin` was removed failing every
    // start after, or started without a fetch, so it never sees the hub's repository move again.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("recover");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    let seed = seeded(&scratch);
    let url = format!("http://127.0.0.1:{port}/git");
    let container = Container { url: &url, bearer: &SESSION.to_string(), secs: 60 };
    let cut_short = scratch.path("home").join("work.cloning");
    assert!(fs::create_dir_all(cut_short.join(".git")).is_ok(), "the test leaves a clone cut short");
    assert!(fs::write(cut_short.join("half"), b"half").is_ok(), "the clone cut short holds a file");
    let work = scratch.path("home").join("work");

    let cloned = served(&mut harness, container.start(&scratch, "home"));

    assert!(cloned.status.success(), "{}", said(&cloned));
    assert_eq!(fs::read(work.join("first")).ok().as_deref(), Some(&b"first"[..]), "main is checked out");
    assert!(!work.join("half").exists(), "nothing of the clone cut short is in the tree");
    assert!(!cut_short.exists(), "the clone cut short is removed");

    let removed = ran(git(&work, &["remote", "remove", "origin"]));
    assert!(removed.status.success(), "{}", said(&removed));
    let later = commit(&seed, "later", b"later");
    let repository = scratch.path("repository.git").display().to_string();
    let pushed = ran(git(&seed, &["push", "--quiet", &repository, "HEAD:refs/heads/main"]));
    assert!(pushed.status.success(), "{}", said(&pushed));
    assert!(fs::remove_file(scratch.path("home").join("claude-ran")).is_ok(), "the test forgets the first run");
    let fetched = served(&mut harness, container.start(&scratch, "home"));
    let origin = ran(git(&work, &["remote", "get-url", "origin"]));

    assert!(fetched.status.success(), "{}", said(&fetched));
    assert_eq!(String::from_utf8_lossy(&origin.stdout).trim(), url, "origin is put back at the hub's repository");
    assert_eq!(named(&work, "refs/remotes/origin/main"), later, "main as the owner moved it is fetched");
    assert_eq!(claude_ran(&scratch, "home").as_deref(), Some("--verbose\n"));

    Ok(())
}

#[test]
fn a_start_script_whose_clone_or_fetch_fails_or_runs_past_its_limit_exits_with_its_code_and_never_runs_claude()
-> Result<(), Unheard> {
    // Catches a bootstrap that fails and carries on, so Claude Code starts half set up; one that ends with git's own
    // code, which the hub reads as a lost container; a step that hangs, as a hub that never answers holds it, with no
    // limit; a failure that does not say why on stderr, which the transcript keeps for the owner; and a work tree
    // that is not a repository taken as one, made one, or cloned over, so its files are changed or lost.
    if !has_git() {
        return Ok(());
    }
    let scratch = Scratch::new("unbootstrapped");
    let (mut harness, port) = hub(scratch.settings(), on_loopback())?;
    seeded(&scratch);
    let url = format!("http://127.0.0.1:{port}/git");
    let session = SESSION.to_string();
    let refused = Container { url: &url, bearer: "no-session", secs: 60 };
    let fetched = Container { url: &url, bearer: &session, secs: 60 };
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("the test binds a port that never answers");
    };
    let address = listener.local_addr().map(|address| address.to_string()).unwrap_or_default();
    let silent = format!("http://{address}/git");
    let hung = Container { url: &silent, bearer: &session, secs: 1 };

    let cloned = served(&mut harness, fetched.start(&scratch, "unfetch"));
    assert!(cloned.status.success(), "{}", said(&cloned));
    assert!(fs::remove_file(scratch.path("unfetch").join("claude-ran")).is_ok(), "the test forgets the clone's run");

    let unrepository = scratch.path("unrepository").join("work");
    assert!(fs::create_dir_all(&unrepository).is_ok(), "the test makes a work tree that is not a repository");
    assert!(fs::write(unrepository.join("notes"), b"notes").is_ok(), "the tree holds a file");

    let unclone = served(&mut harness, refused.start(&scratch, "unclone"));
    let unfetch = served(&mut harness, refused.start(&scratch, "unfetch"));
    let unlimited = served(&mut harness, hung.start(&scratch, "unlimited"));
    let unrepositoried = served(&mut harness, fetched.start(&scratch, "unrepository"));

    for (home, output, why) in [
        ("unclone", unclone, "the clone of"),
        ("unfetch", unfetch, "the fetch of origin failed"),
        ("unlimited", unlimited, "ran past its limit of 1 seconds"),
        ("unrepository", unrepositoried, "is there but is not a git repository"),
    ] {
        let code = output.status.code().map(i64::from);
        assert_eq!(code, Some(BOOTSTRAP_FAILED), "{home}: {}", said(&output));
        assert!(said(&output).contains(why), "{home} says why: {}", said(&output));
        assert!(output.stdout.is_empty(), "{home} prints nothing to stdout, which the hub reads as Claude Code's");
        assert_eq!(claude_ran(&scratch, home), None, "{home} never runs Claude Code");
    }
    let kept: Vec<String> = fs::read_dir(&unrepository)
        .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .map(Iterator::collect)
        .unwrap_or_default();
    assert_eq!(kept, ["notes"], "nothing is added to a tree that is not a repository");
    assert_eq!(fs::read(unrepository.join("notes")).ok().as_deref(), Some(&b"notes"[..]), "its file is untouched");

    Ok(())
}
