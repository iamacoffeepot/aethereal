//! The tools the console's line has called and not yet heard from (ADR-0053: Commands are tools): each call counted
//! as it is asked, and let go as its answer, its refusal, or the tools root's word that it was not answered comes, so
//! the prompt's strip shows that something is under way between the line's echo and its answer.

use aethereal_data::Plain;

/// The tools called and not yet answered, by name, a name once for each of its calls, in the order they were asked.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Running {
    tools: Vec<String>,
}

impl Running {
    /// Counts a call of `tool`, asked.
    pub fn called(&mut self, tool: &str) {
        self.tools.push(String::from(tool));
    }

    /// Lets go of one call of `tool`, as it has been answered. A tool that is not running lets go of nothing.
    pub fn settled(&mut self, tool: &str) {
        if let Some(at) = self.tools.iter().position(|running| running == tool) {
            self.tools.remove(at);
        }
    }

    /// What the prompt's strip shows of the calls: `running read_recent…` for one, `running 2…` for several, and
    /// nothing while none is under way.
    pub fn shown(&self) -> String {
        match self.tools.as_slice() {
            [] => String::new(),
            [tool] => format!("running {tool}…"),
            several => format!("running {}…", several.len()),
        }
    }
}
