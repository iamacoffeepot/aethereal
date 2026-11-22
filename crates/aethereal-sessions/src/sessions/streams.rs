//! A session's streams (ADR-0057): its containers' logs read into its transcript, its bootstrap container's for people
//! to read and its Claude Code container's for what each event of Claude's says of its turns and tasks; its stdin
//! written through its attach; and Docker's events of its containers.

use super::heard::{self, Heard};
use super::record::Conversation;
use super::session::{Activity, Flow, Life, Stdin, Step};
use super::transcript::{Ended, Entry, Note};
use super::{Feed, Sessions, Typing};
use crate::{Attached, Attaching, Detached, Event, EventsDone, Logging, LogsDone, Output, Watching};
use aethereal_actor::{Answered, Fault, Level, Number, Part, Taken, Turn};
use aethereal_tcp::{Write, Wrote};
use core::mem;

impl Sessions {
    /// A piece of a container's output: each line it ends goes into the session's transcript. A line of Claude Code's
    /// says what it says of the session's turns; a bootstrap container's is kept for people to read, and nothing is
    /// read from it.
    pub(super) fn handle_part_logging(&mut self, part: Part<Logging>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Taken { value: Output { stream, bytes }, kept, .. }) = self.logging.part(turn, part)? else {
            return Ok(());
        };
        let Feed { session, run } = *kept;
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let (reading, is_bootstrap) = match &mut held.life {
            Life::Running(running) if running.run == run => (&mut running.reading, false),
            Life::Starting { run: starting, step: Step::Bootstrapping { reading, .. }, .. } if *starting == run => {
                (reading, true)
            }
            Life::Starting { .. }
            | Life::Running(_)
            | Life::Stopping { .. }
            | Life::Hibernating { .. }
            | Life::Archived => {
                return Ok(());
            }
        };
        let entries = reading.read(stream, turn.read(&bytes));
        if is_bootstrap {
            for entry in entries {
                self.note(turn, session, entry);
            }
            return self.flush(turn);
        }
        for entry in entries {
            let (heard, conversation) = match &entry {
                Entry::Claude(json) => heard::read(json.as_str()),
                _ => (Heard::Other, None),
            };
            self.note(turn, session, entry);
            if let Some(id) = conversation {
                self.named(session, id);
            }
            self.heard(turn, session, heard)?;
        }

        self.flush(turn)
    }

    /// A container's logs ended, as they do once it stops: its state is asked for.
    pub(super) fn handle_logs_done(
        &mut self,
        (answered, number): (Answered<LogsDone>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Feed { session, run }) = self.logging.answered(number) else {
            return Ok(());
        };
        if let Answered::Yes(LogsDone::Failed(why)) = &answered {
            turn.log(Level::Warn, format_args!("session {session}'s logs failed: {why:?}"));
        }
        self.closed(turn, session, run, number, Closing::Logs)
    }

    /// The container's stdin, attached: each line waiting is written. Its output is read from its logs instead, so
    /// what the attach carries is taken and let go.
    pub(super) fn handle_part_attaching(
        &mut self,
        part: Part<Attaching>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Taken { value, kept, .. }) = self.attaching.part(turn, part)? else {
            return Ok(());
        };
        let Feed { session, run } = *kept;
        let Attached::Stdin(writing) = value else {
            return Ok(());
        };
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = &mut held.life else {
            return Ok(());
        };
        if running.run != run {
            return Ok(());
        }
        let lines = match mem::replace(&mut running.stdin, Stdin::Open(writing)) {
            Stdin::Attaching { lines } => lines,
            Stdin::Open(_) => Vec::new(),
        };
        for line in lines {
            self.type_raw(turn, session, line)?;
        }

        Ok(())
    }

    /// A container's attach ended: its state is asked for.
    pub(super) fn handle_detached(
        &mut self,
        (_, number): (Answered<Detached>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Feed { session, run }) = self.attaching.answered(number) else {
            return Ok(());
        };

        self.closed(turn, session, run, number, Closing::Attach)
    }
}

/// Which of a container's streams ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closing {
    Logs,
    Attach,
}

impl Sessions {
    /// The stream `number`, `closing`, of `session`'s run `run` ended: when it was the running Claude Code container's,
    /// or the logs of the run's bootstrap container, it is marked closed and the container's state asked for. A
    /// bootstrap container is judged only so, by the state Docker answers, once its logs have ended, as Docker ends
    /// them only once it has stopped and all it wrote is read, so the transcript holds what it said by then.
    fn closed(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        run: u32,
        number: Number,
        closing: Closing,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let running = match &mut held.life {
            Life::Running(running) => running,
            Life::Starting { run: starting, step: Step::Bootstrapping { container, logs, .. }, .. } => {
                let is_run = *starting == run;
                let is_open = *logs == Flow::Open(number);
                let is_current = is_run && is_open && closing == Closing::Logs;
                if !is_current {
                    return Ok(());
                }
                *logs = Flow::Closed;
                let container = container.clone();
                return self.inspect(turn, session, container, run);
            }
            Life::Starting { .. } | Life::Stopping { .. } | Life::Hibernating { .. } | Life::Archived => {
                return Ok(());
            }
        };
        let is_run = running.run == run;
        let flow = match closing {
            Closing::Logs => &mut running.logs,
            Closing::Attach => &mut running.attach,
        };
        let is_open = *flow == Flow::Open(number);
        let is_current = is_run && is_open;
        if !is_current {
            return Ok(());
        }
        *flow = Flow::Closed;
        if closing == Closing::Attach {
            running.stdin = Stdin::Attaching { lines: Vec::new() };
        }
        let container = running.container.clone();

        self.inspect(turn, session, container, run)
    }

    /// Claude named `session`'s conversation `id`: it is recorded, for the next container to resume.
    fn named(&mut self, session: u32, id: String) {
        let Some(held) = self.held.get_mut(&session) else {
            return;
        };
        let named = Conversation::Begun(id);
        if held.conversation != named {
            held.conversation = named;
            self.dirty(session);
        }
    }

    /// What an event of Claude's in `session` says of its turns and tasks, and what it asks the owner.
    fn heard(&mut self, turn: &mut Turn<'_, Self>, session: u32, heard: Heard) -> Result<(), Fault> {
        let heard = match heard {
            Heard::Asked { request, tool, input } => return self.asked(turn, session, request, &tool, input),
            other => other,
        };
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = &mut held.life else {
            return Ok(());
        };
        match heard {
            Heard::Init => match running.activity {
                Activity::Idle { .. } | Activity::Tasked => {
                    running.activity.cancel(turn);
                    running.activity = Activity::Working;
                }
                Activity::Working | Activity::Noticed { .. } | Activity::Compacting { .. } => {}
            },
            Heard::Result => match running.activity {
                Activity::Noticed { .. } => return self.compact(turn, session),
                Activity::Compacting { .. } => return self.wound_down(turn, session),
                Activity::Working | Activity::Tasked | Activity::Idle { .. } => return self.settle(turn, session),
            },
            Heard::TaskStarted(task) => {
                running.tasks.insert(task);
                if let Activity::Idle { .. } = running.activity {
                    running.activity.cancel(turn);
                    running.activity = Activity::Tasked;
                }
            }
            Heard::TaskEnded(task) => {
                running.tasks.remove(&task);
                if matches!(running.activity, Activity::Tasked) {
                    return self.settle(turn, session);
                }
            }
            Heard::Compacting | Heard::CompactBoundary | Heard::Status | Heard::Asked { .. } | Heard::Other => {}
        }

        Ok(())
    }

    /// Writes `text` to `session`'s stdin as a user's message, once its stdin is attached.
    pub(super) fn type_line(&mut self, turn: &mut Turn<'_, Self>, session: u32, text: &str) -> Result<(), Fault> {
        self.type_raw(turn, session, user_line(text))
    }

    /// Writes the interrupt control request to `session`'s stdin, numbered `number`.
    pub(super) fn type_interrupt(&mut self, turn: &mut Turn<'_, Self>, session: u32, number: u64) -> Result<(), Fault> {
        let line = format!(
            r#"{{"type":"control_request","request_id":"interrupt-{number}","request":{{"subtype":"interrupt"}}}}"#
        );

        self.type_raw(turn, session, line + "\n")
    }

    /// Writes `line`, whole, to `session`'s stdin, or keeps it until its stdin is attached.
    pub(super) fn type_raw(&mut self, turn: &mut Turn<'_, Self>, session: u32, line: String) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return Ok(());
        };
        let Life::Running(running) = &mut held.life else {
            return Ok(());
        };
        let writing = match &mut running.stdin {
            Stdin::Open(writing) => *writing,
            Stdin::Attaching { lines } => {
                lines.push(line);
                return Ok(());
            }
        };
        let Ok(part) = turn.put(line.into_bytes()) else {
            let why = String::from("the engine had no room for the line");
            self.note(turn, session, Entry::Hub(Note::Unwritten { why }));
            return Ok(());
        };
        turn.to(writing).ask(&Write { parts: vec![part] }, &Typing { session })?;

        Ok(())
    }

    /// A write to a session's stdin: one that did not go is noted in its transcript.
    pub(super) fn handle_wrote(
        &mut self,
        (answered, Typing { session }): (Answered<Wrote>, Typing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let why = match answered {
            Answered::Yes(Wrote::Sent) => return Ok(()),
            Answered::Yes(wrote) => format!("{wrote:?}"),
            Answered::No(why) => format!("{why:?}"),
        };
        self.note(turn, session, Entry::Hub(Note::Unwritten { why }));

        self.flush(turn)
    }

    /// An event of a container carrying the hub's label: a session's Claude Code container that died has ended, and is
    /// lost. A bootstrap container that dies is judged once its logs end, which follows, from Docker's state of it, so
    /// what it wrote is in the transcript first.
    pub(super) fn handle_part_watching(
        &mut self,
        part: Part<Watching>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Taken { value, .. }) = self.watching.part(turn, part)? else {
            return Ok(());
        };
        match value {
            Event::Died { container, code } => {
                let running = self.held.iter().find_map(|(session, held)| match &held.life {
                    Life::Running(running) if running.container == container => Some((*session, running.run)),
                    _ => None,
                });
                if let Some((session, run)) = running {
                    self.ended(turn, session, run, Ended::Exited { code })?;
                }
            }
            Event::OutOfMemory { container } => {
                turn.log(Level::Warn, format_args!("a process of container {} ran out of memory", container.as_str()));
            }
            Event::Started { .. } => {}
        }

        self.flush(turn)
    }

    /// Docker's events ended: a container that dies is then found as its logs end.
    #[allow(clippy::unnecessary_wraps, reason = "a handler's body, it takes and answers what its handler does")]
    pub(super) fn handle_events_done(
        &mut self,
        (answered, number): (Answered<EventsDone>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.watching.answered(number);
        turn.log(Level::Warn, format_args!("Docker's events of the sessions' containers ended: {answered:?}"));

        Ok(())
    }
}

/// The line of stream-json that says `text` as a user's message.
pub(super) fn user_line(text: &str) -> String {
    let mut line = String::from(r#"{"type":"user","message":{"role":"user","content":"#);
    aethereal_data::write_json_string(text, &mut line);
    line.push_str("}}\n");

    line
}
