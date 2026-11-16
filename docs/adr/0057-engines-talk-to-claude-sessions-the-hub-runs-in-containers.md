# ADR-0057: Engines talk to Claude sessions the hub runs in containers

- **Status:** Proposed
- **Date:** 2026-11-13

## Context

The engine's owner works with Claude from outside the engine today: Claude drives engines through the hub's MCP tools (ADR-0037, ADR-0038: `EngineTools`), and the owner reads the conversation somewhere else. The aim is for the owner to sit in an engine and work with Claude from its dev overlay (ADR-0053), with Claude's sessions running beside the hub on the build host.

Constraints:

- **The owner's Claude subscription, not the API.** Claude Code's print mode, `claude -p`, runs on a subscription token from `claude setup-token`. A subscription is one person's, so the hub is a local tool for one person on one machine.
- **The engine stays thin.** An engine shows a session and sends what its owner does; it never runs Claude, holds a credential, or knows how a session is hosted.
- **Sessions outlive engines.** Engines are restarted, republished, and closed all the time, and a conversation must not end with one.
- **A session runs whatever it wants, within limits.** It builds, runs tests, and spawns engines, so it needs its own files and a share of the build host that the hub can cap.

What is already decided:

- **Only tools cross the link** (ADR-0038: `Up::{Hello, Sheets, Called}`, `Down::{Refused, Call}`, `REVISION`). The hub calls an engine's tools, and an engine never asks the hub for anything.
- **The hub spawns engines from its store** (ADR-0039, ADR-0040: `CheckIn`, `Launch`, `Spawned`, `MintToken`). `check_in` reads a path on the hub's machine.
- **The dev overlay's console reads a typed line into one of the engine's tools** (ADR-0053: `CommandLine`, `print_value`).
- **HTTP is a server on the TCP capability** (ADR-0035: `Http`, `Serve`; ADR-0033: `Tcp`). There is no client and no Unix socket.

What a probe on the build host established, with Claude Code 2.1.310 in a container reached through Docker's Engine API over its Unix socket:

- `claude -p --input-format stream-json --output-format stream-json --verbose` runs headless on a subscription token passed as `CLAUDE_CODE_OAUTH_TOKEN`. Two containers ran at once, and a container reached the hub's MCP server.
- **Attach upgrades.** `POST /containers/{id}/attach?stream=1&stdin=1&stdout=1&stderr=1` with `Upgrade: tcp` is answered `101 UPGRADED`, and the connection is then the process's stdin and stdout, each output chunk behind an 8-byte header naming its stream and length.
- **Stdin outlives an attach.** With `OpenStdin` set and `StdinOnce` clear, a detached and re-attached connection reaches the same process, which remembers the conversation.
- **A turn finishes with nobody attached,** and `GET /containers/{id}/logs?since=` returns all of it.
- **Interrupt is a line on stdin:** `{"type":"control_request","request_id":…,"request":{"subtype":"interrupt"}}` is answered by a `control_response`, the running tool is stopped, the turn ends as `error_during_execution`, and the process takes the next turn.
- **Subagents run in the session's process.** Their events carry `parent_tool_use_id`, with `system` events `task_started`, `task_progress`, `task_updated`, and `task_notification`.
- **A background subagent outlives its turn, and when it finishes, Claude starts a turn by itself:** `init`, the assistant's message, and a `result`, with no input sent.
- **`/compact` sent as a message compacts:** `system` `status: compacting`, then `compact_boundary` with `trigger`, `pre_tokens`, and `post_tokens`. The conversation carries on from the summary.
- **`--resume <id>` in a new container** with the same home volume continues the session.
- Print mode chose Sonnet by default, so the model must be stated.

## Decision

**The hub runs each open Claude session in a Docker container of its own, made only from the hub's config and reached through Docker's Engine API over its Unix socket. A new capability, `Sessions`, holds the sessions, writes the owner's lines to a session's stdin, reads its events from its stdout, and keeps each session's events as one numbered transcript. Trusted engines, the ones the owner starts, gain chat frames on the link: they list every session as a tab, open one to read its transcript from any point, type to it, interrupt it, and answer what it asks. Untrusted engines, the ones a session spawns, keep the link as it is. The dev overlay's drop-down console becomes the chat: a line goes to Claude, and a line starting with `/` runs the engine's own tool, as it does today.**

```
console line ──> Up::Say ──> hub: Engine(37) tags it ──> Session: numbered transcript
                                                            └──> attach stream (stdin) ──> claude -p in the container
claude -p ──> stdout events ──> logs, followed ──> Session: numbered transcript ──> Down::Show to every engine with the tab open
claude -p ──> hub MCP (its session's route) ──> check_in, spawn_engine, call_tool on its work engines
console /line ──> the engine's own tool, now ──> Up::Ran ──> transcript, folded into the next Say
```

### A session is a container

- **One container per open session.** It runs `claude -p --input-format stream-json --output-format stream-json --verbose --model <model> --dangerously-skip-permissions`, with `--resume <id>` once Claude has named its conversation, and lives while the session is open: a long build or a background subagent keeps running between turns. The id is read from the `session_id` Claude's events carry, and is passed on only when it is a UUID.
- **Each container of a session is a run,** numbered from 1 and named for the hub's label, the session, and the run. A session stopped and resumed runs in a new container on the same home, which is how `--resume` finds the conversation.
- **The container is built only from the capability's config.** Its image, its mounts, its limits, its network, and its labels all come from `Sessions`' settings and the session's number and run. Nothing is passed through from a tool call, a frame, or Claude, so no one can ask for another image or another mount: a session's name is only shown, and Claude's conversation id is passed on only as a UUID. Access to the socket is access to the build host, and this rule is what keeps it the hub's.
- **Every container carries two labels:** `aethereal.sessions`, whose value names the hub, which a list and the events are filtered by, and `aethereal.session`, the session's number. The settings may add more, as a test's runs add `aethereal.test=1`.
- **What a container holds:**
  - its own clone of the project, its own cargo target directory, and its own home, each a directory under the sessions root on the build host, bind-mounted. A bind is written as one of `HostConfig.Mounts`, its source and target apart, so no path is split at a colon, and Docker refuses a source that is missing rather than making it, so the capability makes each directory before the container;
  - the shared cargo registry and the build host's cargo lock, mounted;
  - the credential, from a file of `KEY=VALUE` lines the capability's settings name, read as each container is made and given as its environment. The Engine API has no environment file, so the value is in the create's body; it never appears in an argument, a log, or a record, and a variable's `Debug` names it without its value;
  - the Claude Code binary, from the build host, read-only, at `/usr/local/bin/claude`.
- **The first image is a stock Rust image with the build host's Claude Code bound in,** and each session's home at `/session/home`, its `HOME`. The project's image, its clone, its target directory, and the shared cargo mounts come with the session image (see Order of work).
- **Permission checks are skipped inside the container** (`--dangerously-skip-permissions`). The container is the sandbox: it holds no git credential and no key of the owner's, so what Claude does in it stays in it until something the hub serves takes it out.
- **The model is always stated,** from the settings, as print mode otherwise picks its own.
- **Limits are the hub's.** Each container is capped by CPU and memory (`HostConfig.NanoCpus`, `Memory`), and at most a set number of sessions run at once across the hub; a new session, or a line that would resume a stopped one, past it is refused with an error that names the limit and the sessions running. Stopped and archived sessions do not count. Defaults: 4 CPUs and 8 GiB each, 4 running, 30 minutes idle.
- **An idle session is wound down, then stopped.** A session with no turn running and no background task of its running (`task_started` until its `task_notification`) is idle; once it has been idle for the idle time:
  1. if it has had a turn since its last `compact_boundary`, the hub writes it a notice, a user's line tagged in the transcript as the hub's, whose text is a setting: it has been idle for that many minutes, it will be compacted next, and it should write down its loose threads, unfinished work, open questions, uncommitted changes, and what to pick up next. The hub waits for that turn's `result`;
  2. it writes `/compact`, also the hub's, and waits for its `result`;
  3. it stops the container and removes it, keeping the session's home.

  Each wait has a deadline, a setting; past it, the transcript notes which step was overdue and the container is stopped anyway. A line said during steps 1 and 2 makes the session active again: the rest is cancelled, and the session is not stopped. A session with nothing new since its last compaction goes straight to step 3. The next line said to a stopped session resumes it in a new container, with `--resume`.
- **Archiving a session** stops and removes its container and keeps its transcript and home on disk; an archived session never runs again.
- **A session's builds reach the store through the mount.** Its target directory is a path on the hub's machine, so `check_in` takes a built engine from it as it takes any path, and `spawn_engine` runs it.

### The hub reaches Docker through its Unix socket

- **Docker's Engine API is HTTP over `/var/run/docker.sock`.** The hub uses it directly, not the `docker` command:
  - `POST /containers/create`, `/start`, `/stop`, `/kill`, `DELETE /containers/{id}`, and `GET /containers/{id}/json` to make, end, and inspect a session's container;
  - `GET /containers/json`, filtered by the label every session container carries, to count what runs;
  - `POST /containers/{id}/attach`, upgraded, for stdin and stdout;
  - `GET /containers/{id}/logs?follow=1` for stdout and stderr, from the container's start;
  - `GET /events` to learn at once that a container died.
- **Three things are built for it, each in the crate it belongs to:**
  - **a Unix transport** beside `Tcp`, a stream connection to a socket path;
  - **an HTTP/1.1 client in `aethereal-http`:** requests with `Content-Length` bodies, responses with `Content-Length` or chunked bodies, and an upgrade that hands the connection back as a raw stream. It is written for the subset Docker speaks, not taken from a crate;
  - **a Docker client in the sessions capability's crate, `aethereal-sessions`:** a root, `Docker`, asked at its typed path. It answers typed requests (`Create` from a `ContainerSpec`, `Start`, `Stop`, `Kill`, `Remove`, `Inspect`, `List` by a label) and serves typed streams (`Attaching`, whose first part is the container's stdin and each later part its output; `Logging` from a time on; `Watching` the start, die, and OOM events of the containers that carry a label), each paced by its asker's credit, with the 8-byte stream frames of an attach and of logs taken apart into stdout and stderr. No request, path, or JSON of its asker's reaches Docker: a container is made only from a `ContainerSpec`, and named only by its id as Docker gives one, never by a name or a path. Each request goes on a connection of its own, which the client hangs up once its reply is read, as `Http`'s client takes one request at a time and an attach or the events hold theirs for hours. Every path names API version 1.44, so a daemon answers in the shapes the client reads. Its errors are named cases: not found (`404`), a conflict (`409`), another status, a socket missing or with nothing listening, and a malformed reply.
- **The container's stdin and stdout belong to Docker, not to the hub.** A hub that restarts loses no turn: the container keeps working, and the hub re-attaches and reads what it missed from the logs.
- **A session's output is read from its logs, followed from its container's start, and its stdin written through its attach.** The transcript knows how many bytes of the latest container's stdout and stderr it holds, so a logs stream opened again, after the hub restarted or a stream ended while the container runs, skips them and records each line once, with none missed between an attach and a catch-up. What the attach carries of the output is let go. A container's logs ending, or its attach, asks Docker for its state: one still running is followed again, and one that is not has ended.
- **Every ask of Docker carries a deadline,** a setting, past which the capability takes it as unanswered (ADR-0041); a stop's deadline adds its grace.

### Sessions belong to the hub

- **A session is a task, not an engine.** Every trusted engine sees every session, and a session is used from whichever engine the owner is at. Several tabs are several tasks.
- **When an engine closes,** only its open tabs end. A turn running carries on, a line already sent is still delivered, and a question waiting for an answer stays waiting until an engine opens the session and answers it.
- **The capability keeps the list:** each session's number, name, conversation id, runs, and where it stood, as a record beside its transcript and its home, in a directory of its own under the sessions root. A hub that starts again reads them, and then finds their containers: it follows Docker's events for its label before it lists the containers that carry it, so nothing that happens to one falls between the two; it re-attaches to each container a record says may run that still does, notes each that does not as lost, and reports each carrying its label that it has no record of, never removing it. Asks that come while it does wait for it.
- **The transcript is the hub's own record,** not Claude Code's session file, which is Claude Code's format and may change. The hub keeps every event, numbered in order within its session from 1, and appended to the session's file as it comes, so a read after a restart goes on from the same numbers: each line of the container's stdout, as Claude's event when it is JSON; each line of its stderr; each line written to it, tagged with who said it, a caller of the hub's tools or the hub itself, and later the engine; each interrupt; and what the hub did with the container. A turn Claude starts by itself, as a background subagent finishes, is recorded as any other.

### The link carries chat for trusted engines

`REVISION` is raised, and the link gains frames. The hub's `Engine` actor adds the engine's id to what it forwards, so a frame names no engine and none can claim to be another.

| Up (engine → hub) | Means |
|---|---|
| `Say { session, text }` | a line typed without `/` |
| `Ran { line, printed }` | a `/` command the owner ran here, and what it printed |
| `Answer { ask, allow }` | an approval answered |
| `Interrupt { session }` | stop the session's turn |
| `Open { session, after }` / `Close { session }` | a tab opened, from event `after`, or closed |
| `New { name }` / `Archive { session }` | a session made, or ended |

| Down (hub → engine) | Means |
|---|---|
| `Sessions { list }` | every session's name and state, sent whole on every change |
| `Show { session, events }` | events from the transcript, the backlog first, then each as it comes |
| `Ask { ask, what }` | something waits for the owner's answer |

- **The engine sends only what its owner did.** Everything about the engine that Claude needs, a frame, a log, its stats, Claude asks for through the tools that already cross the link. Nothing is pushed up unasked.
- **The list is sent whole.** It is a few dozen entries at most, which is cheaper than tracking what changed, as `Sheets` is.
- **Events are numbered within their session.** `Open { after }` returns every event after `after`, so a fresh engine reads from the start or from the latest, and one that linked again reads exactly what it missed. Earlier events are pulled as the owner scrolls back.
- **Kept by each engine for itself,** and never sent: which tab is open, where it is scrolled, and a line half typed.

### Trusted and untrusted engines

- **An engine the owner starts is trusted.** An engine a session spawns or mints a token for is untrusted. The hub decides when the engine links, from who asked for its token or its spawn, and records it on the engine's `Engine` actor. The engine never says what it is.
- **Each session reaches the hub's tools by a route of its own,** so the hub tells a session's `mint_token` and `spawn_engine` from the owner's. The route is a second MCP port, bound where containers reach it and named only to sessions, on which every call bears a token the hub mints for its session and gives its container in its environment, as it gives the credential: the token names the session, and a call with none or another is refused. The owner's port stays on the loopback, which a container on a network of its own does not reach. Until the route is served, alongside the trusted engines, a container joins the host's network and reaches the owner's port, and nothing tells a session's calls apart.
- **An untrusted engine keeps the link as it is:** it lists its tools and answers the hub's calls, which is everything a session needs to test it. Every chat frame from it is dropped and logged, and none is sent to it. Its overlay shows no chat.
- **Why:** a work engine runs code a session has just written, its link included. A half-made build that sent a line every frame, resent an old `Answer`, or let one session's text reach another as if typed, would act on real sessions. Only the hub's check holds against code the engine runs.

### The console is the chat

- **One stream per session, in every engine.** Lines from every engine, Claude's answers, and commands the owner ran are interleaved in order, each line tagged with its engine, `[37]`. An engine shows its own lines fully and others' dimmed; it filters nothing, since Claude saw one conversation.
- **A line goes to Claude, and a line starting with `/` runs here.** A `/` line runs the engine's own tool at once, during a turn or not, as ADR-0053 reads it, and never waits on Claude. The engine sends it as `Ran`, which goes into the transcript without starting a turn, and the hub folds every `Ran` since the owner's last line into the head of their next one, so Claude knows what was done.
- **Claude Code's own commands are passed through.** A short list of `/` words, `/compact` among them, belongs to the session and is sent as a line.
- **Esc interrupts.** With the prompt line empty, Esc, or `/stop`, sends `Interrupt`, and the hub writes the interrupt control request to the session's stdin. With text in the prompt line, Esc clears it.
- **Lines sent during a turn go in at once,** and Claude Code queues them itself, answering an interrupt with what it still holds (`still_queued`). The transcript records each where it was said, so it reads in the order the owner typed.
- **A turn may start with no line,** when a background subagent finishes. The hub records it as any turn, and the tab shows it working.
- **Compaction shows, and loses nothing.** A `compact_boundary` event becomes a divider in the transcript with its token counts, and the tab shows the session busy while `status` is compacting. The transcript keeps every event before it.
- **Subagents fold.** Events carrying `parent_tool_use_id` are shown under the tool call that started their subagent, closed until opened.
- **The console drops down from the top, as it does now,** grown to hold the transcript. The prompt line is at its bottom edge and the session tabs under it, nearest the scene, with a mark for a session working or waiting on the owner; a waiting session shows on the band too. The console's height is set by dragging and kept per engine.

## Consequences

- **The owner works from an engine:** one engine they sit in, sessions in its console, and work engines that sessions spawn, restart, and test without ever restarting the owner's.
- **Sessions are contained.** A session's files, builds, and processes stay in its container and its directories, its share of the build host is capped, and two sessions never write the same tree.
- **The hub is the one place sessions live.** A hub restart costs no turn, and an engine restart costs nothing.
- **New crates and crate code:** the Unix transport, the HTTP client in `aethereal-http`, the sessions capability, `aethereal-sessions`, with its Docker client, and session tools in the hub's toolbox (`session_new`, `session_say`, `session_read`, `session_list`, `session_interrupt`, `session_archive`) so sessions are used and tested before the overlay has a chat.
- **The link's revision changes,** so an old engine and a new hub refuse each other at `Hello`.
- **The widget library has gaps the chat needs:** a text input of several lines, a clipboard (which the window does not have), markdown turned into widgets, images inline in the transcript, a scroll that stays cheap over a long transcript (the library's draw stage), and a surface for approvals.
- **The build host's Docker keeps its data on the disk that holds the sessions root,** not the system disk, before sessions run.
- **Each session is told where it is:** the capability appends a short prompt saying it runs in a container, what `[37]` means, that the hub's tools are how it spawns and tests engines, and that it never changes the owner's own engine or its window.
- **Order of work:**
  - A, the hub: the Unix transport; the HTTP client; the Docker client; `Sessions` and its tools; the session image and prompt.
  - The sessions' own route to the hub's tools comes with the trusted engines, in B.
  - B, the engine: the library's draw stage; text editing and the clipboard; markdown and images; the link's chat frames and trusted engines (after `Sessions`); the overlay's chat (last).

## Open questions

- **How work leaves a session.** A session holds no git credential. The leading option is a bare repository on the build host, mounted into every container, where a hook lets each session push only its own branches, and from which the owner pushes to GitHub. How local review fits is not settled. Until it is, work leaves only when the owner takes it from the session's clone.
- **The image.** The suggestion is one per project, from a Dockerfile in that project's repository, with its toolchain, git, and no Claude binary of its own. Not yet confirmed.
- **The Desktop app.** Reaching a container session from Claude's desktop app is on hold, as is Claude sending the owner a message at whichever engine they are at.

## Alternatives considered

- **The Anthropic API, with the hub running the agent loop:** the hub would own every step, but the owner works on a subscription, and the API is billed apart.
- **The engine running `claude` itself, through `Processes`:** the session would end with the engine, every engine would need Claude Code and a credential, and several engines could not share one conversation.
- **One session per engine:** engines come and go, a conversation spanning engines would be cut, and every session costs its own context on the subscription.
- **`claude -p` directly on the build host, no container:** simpler, but sessions would share one tree and the whole machine, and each command would need the owner's approval rather than the few that leave the container.
- **A container per turn:** nothing to keep alive, but a build or a background subagent left running would die at the turn's end.
- **The `docker` command through `Processes` with pipes:** works, but a hub restart closes the pipe and stops the container's process, and every answer is printed text to parse. The API gives structured state and an event stream, and leaves the pipes with Docker.
- **Docker's API on a TCP port:** anyone who reaches the port controls the build host.
- **A relay inside the container listening on a socket on the mount:** a program to write and keep, for what attach and logs already do.
- **Named pipes on the mount:** `claude` exits when the last writer closes, and is killed writing to a pipe with no reader, so the hub's restart ends the session.
- **The overlay calling the hub's MCP server for chat:** each engine would need an MCP client, a second connection, and a second credential, where the link already proves which engine it is.
- **Engines pushing captures and scene state to the hub:** it duplicates tools Claude can already call, and sends what nobody asked for.
- **Filtering each engine's view to its own lines:** Claude saw one conversation, and answers about another engine's command would arrive with nothing they refer to.
- **Several people on one hub:** one person's subscription cannot serve others, and serving several would mean API keys, owners on sessions, and sharing. It is a redesign if it is ever wanted, and nothing here is built for it.
- **A session that starts other sessions, each in its own container:** the shape of an autonomous pipeline, out of scope; a session's subagents run in its own container.
