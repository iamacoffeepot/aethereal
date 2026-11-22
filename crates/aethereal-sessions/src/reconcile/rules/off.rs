//! A session with no container: it starts its next run when it should be awake, and wakes when an owner's line was
//! said after its hold.

use super::wake;
use crate::reconcile::effect::{Effect, Until};
use crate::reconcile::facts::Facts;
use crate::reconcile::record::{Desired, How, Last, Phase, Reason, Record, Run, When};
use core::ops::ControlFlow::{self, Break};

pub(super) fn off(f: &Facts<'_>) -> ControlFlow<Effect> {
    let record = f.record;
    wake(f)?;
    if record.desired != Desired::Awake {
        return Break(Effect::Wait(Until::Event));
    }
    let run = record.run + 1;
    if !f.served {
        let how = How::Unstarted(Reason::Repository);
        let desired = Desired::Hibernating(When::AfterTurn);
        let last = Last::Ended { run, how };
        return Break(Effect::Record(Record { run, last, desired, held_through: f.ledger.entries(), ..*record }));
    }

    Break(Effect::Record(Record { run, phase: Phase::Bootstrap(Run::Running), ..*record }))
}
