//! An idle session wound down (ADR-0057): once it has had no turn and no background task for the idle time, a session
//! that has had a turn since it was last compacted is told it is about to be compacted, so it writes down its loose
//! threads, then compacted, and then stopped. A line said meanwhile makes it active again, and each step that takes
//! past its deadline is noted, and the session stopped anyway.

use super::session::{Activity, Life, Then};
use super::transcript::{Ended, Entry, Note, Since, Speaker, WindDown};
use super::{Resting, Sessions};
use aethereal_actor::{Answer, Answered, Fault, JobId, Receives, Sleep, Slept, Turn};
use core::time::Duration;

/// The line that asks Claude Code to compact its session.
const COMPACT: &str = "/compact";

impl Sessions {
    /// `session`'s turn ended, or its last task: it is idle once no task runs, and waits the idle time.
    pub(super) fn settle(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let rest = self.rest();
        let idle_secs = self.settings.idle_secs;
        let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) else {
            return Ok(());
        };
        running.activity.cancel(turn);
        let is_tasked = !running.tasks.is_empty();
        if is_tasked {
            running.activity = Activity::Waiting;
            return Ok(());
        }
        let sleep = wait(turn, session, rest, idle_secs)?;
        running.activity = Activity::Idle { sleep, rest };

        Ok(())
    }

    /// The notice's turn ended: the session is asked to compact, within the deadline.
    pub(super) fn compact(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let rest = self.rest();
        let wind_down_secs = self.settings.wind_down_secs;
        let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) else {
            return Ok(());
        };
        running.activity.cancel(turn);
        let deadline = wait(turn, session, rest, wind_down_secs)?;
        running.activity = Activity::Compacting { deadline, rest };
        self.note(session, Entry::Said { by: Speaker::Hub, text: String::from(COMPACT) });

        self.type_line(turn, session, COMPACT)
    }

    /// The compaction's turn ended: the session is stopped.
    pub(super) fn wound_down(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        self.stop(turn, session, Ended::Idle, Then::Rest { lines: Vec::new() })
    }
}

impl Receives<Answer<Slept, Resting>> for Sessions {
    /// A session's idle time, or a step's deadline, is up: unless it was active since, it is wound down a step.
    fn handle(
        &mut self,
        (answered, Resting { session, rest }): (Answered<Slept>, Resting),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Slept) = answered else {
            return Ok(());
        };
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };
        let Life::Running(running) = &held.life else {
            return Ok(());
        };
        let since = held.transcript.since();
        let due = match running.activity {
            Activity::Idle { rest: idle, .. } if idle == rest => Due::Idle,
            Activity::Noticed { rest: noticed, .. } if noticed == rest => Due::Overdue(WindDown::Notice),
            Activity::Compacting { rest: compacting, .. } if compacting == rest => Due::Overdue(WindDown::Compaction),
            Activity::Idle { .. }
            | Activity::Noticed { .. }
            | Activity::Compacting { .. }
            | Activity::Working
            | Activity::Waiting => Due::Passed,
        };
        match (due, since) {
            (Due::Idle, Since::Turned) => self.notice(turn, session)?,
            (Due::Idle, Since::Compacted) => self.wound_down(turn, session)?,
            (Due::Overdue(step), _) => {
                self.note(session, Entry::Hub(Note::Overdue { step }));
                self.wound_down(turn, session)?;
            }
            (Due::Passed, _) => {}
        }

        self.flush(turn)
    }
}

impl Sessions {
    /// Tells `session`, idle, that it is about to be compacted, and waits for that turn within the deadline.
    fn notice(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let rest = self.rest();
        let wind_down_secs = self.settings.wind_down_secs;
        let notice = self.settings.notice();
        let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) else {
            return Ok(());
        };
        let deadline = wait(turn, session, rest, wind_down_secs)?;
        running.activity = Activity::Noticed { deadline, rest };
        self.note(session, Entry::Said { by: Speaker::Hub, text: notice.clone() });

        self.type_line(turn, session, &notice)
    }
}

/// What a wait on the timer came to, as the session stands.
enum Due {
    /// Its idle time is up.
    Idle,
    /// The deadline of this step is up.
    Overdue(WindDown),
    /// It was active since, or the wait is not its latest.
    Passed,
}

/// Waits `secs` on the timer for `session`'s rest `rest`.
fn wait(turn: &mut Turn<'_, Sessions>, session: u32, rest: u64, secs: u32) -> Result<JobId, Fault> {
    let until = turn.now() + Duration::from_secs(u64::from(secs));

    Ok(turn.run(&Sleep { until }, &Resting { session, rest })?)
}
