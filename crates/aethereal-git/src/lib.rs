//! The git capability (ADR-0057): one root, [`Git`], a handler of an HTTP stack (ADR-0035) that serves one bare
//! repository over git's smart HTTP, and the hook that lets a session push only its own branches.
//!
//! A program routes git's three requests to `Git` on a stack whose layers attach who sent each, a [`Caller`]: `GET
//! .../info/refs?service=<service>`, `POST .../git-upload-pack`, and `POST .../git-receive-pack`. For each, the root
//! starts `git http-backend` through `Processes` (ADR-0040), with an environment it states whole and its stdin and
//! stdout connected; streams the request's body into the program's stdin as it arrives, paced by what the pipe has
//! taken; reads the CGI head the program writes; and answers with that head and a body streamed from itself as the
//! program writes it, paced by what the stack has sent. A client that stops reading holds the program back, and one
//! that goes stops it.
//!
//! The program sees the session as `REMOTE_USER`, `session-<n>`, and runs the hooks of the directory the settings name,
//! as git's `core.hooksPath`: the crate's `hooks/pre-receive` refuses every push of a ref outside the session's own
//! branches, `sessions/<n>/`. As the root begins it makes the repository, `git init --bare`, which leaves one already
//! there as it is, and checks that the hook is in place; until both are done every request is answered `503`, and if
//! either cannot be, the root logs why and answers every request `503` from then on, and the rest of the program runs
//! on.
//!
//! The root runs on native hosts only, as `Processes` does. Its settings, the section `git`, are its config.
//!
//! [`Caller`]: aethereal_tools::Caller

mod call;
mod git;
mod programs;
mod settings;

pub use call::{Call, Service};
pub use git::{Git, HearReadiness, Hearing, Launch, Readiness, Unopened, Writing};
pub use settings::GitSettings;
