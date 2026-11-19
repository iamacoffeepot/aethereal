//! A session wound down to hibernate (ADR-0057): once it has had no turn and no background task for the idle time, or
//! when `hibernate_session` asks, a session that has had a turn since it was last compacted is told it is about to be
//! compacted and hibernated, so it writes down its loose threads, then compacted, and then its container stopped, so it
//! hibernates; one with nothing new since its last compaction has its container stopped at once. A line said meanwhile
//! makes it active again, and each step that takes past its deadline is noted, and the container stopped anyway.

use super::session::{Activity, Gone, Life, Then};
use super::transcript::{Ended, Entry, Note, Since, Speaker, WindDown};
use super::{Resting, Sessions};
use aethereal_actor::{Answered, Fault, JobId, Sleep, Slept, Turn};
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
            running.activity = Activity::Tasked;
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
        self.note(turn, session, Entry::Said { by: Speaker::Hub, text: String::from(COMPACT) });

        self.type_line(turn, session, COMPACT)
    }

    /// The compaction's turn ended: the session's container is stopped, so it hibernates.
    pub(super) fn wound_down(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let then = Then::Hibernate { opening: Vec::new(), lines: Vec::new(), gone: Gone::Removed };

        self.stop(turn, session, Ended::Hibernated, then)
    }

    /// Winds `session`, which runs and is idle, down to hibernate: told it is about to be compacted when it has had a
    /// turn since its last compaction, and its container stopped at once when it has not.
    pub(super) fn wind_down(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(());
        };

        match held.transcript.since() {
            Since::Turned => self.notice(turn, session),
            Since::Compacted => self.wound_down(turn, session),
        }
    }

    /// A session's idle time, or a step's deadline, is up: unless it was active since, it is wound down a step.
    pub(super) fn handle_slept(
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
        let due = match running.activity {
            Activity::Idle { rest: idle, .. } if idle == rest => Due::Idle,
            Activity::Noticed { rest: noticed, .. } if noticed == rest => Due::Overdue(WindDown::Notice),
            Activity::Compacting { rest: compacting, .. } if compacting == rest => Due::Overdue(WindDown::Compaction),
            Activity::Idle { .. }
            | Activity::Noticed { .. }
            | Activity::Compacting { .. }
            | Activity::Working
            | Activity::Tasked => Due::Passed,
        };
        match due {
            Due::Idle => self.wind_down(turn, session)?,
            Due::Overdue(step) => {
                self.note(turn, session, Entry::Hub(Note::Overdue { step }));
                self.wound_down(turn, session)?;
            }
            Due::Passed => {}
        }

        self.flush(turn)
    }
}

impl Sessions {
    /// Tells `session` that it is about to be compacted and hibernated, and waits for that turn within the deadline:
    /// the wait on its idle time is let go, as its wind-down may begin before that is up.
    fn notice(&mut self, turn: &mut Turn<'_, Self>, session: u32) -> Result<(), Fault> {
        let rest = self.rest();
        let wind_down_secs = self.settings.wind_down_secs;
        let notice = self.settings.notice.clone();
        let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) else {
            return Ok(());
        };
        running.activity.cancel(turn);
        let deadline = wait(turn, session, rest, wind_down_secs)?;
        running.activity = Activity::Noticed { deadline, rest };
        self.note(turn, session, Entry::Said { by: Speaker::Hub, text: notice.clone() });

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
