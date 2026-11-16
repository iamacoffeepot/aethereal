//! The chat in the console (ADR-0057: The console is the chat): the sessions the hub lists as tabs, the transcript of
//! each tab opened, the asks that wait on the owner, and which tab is open. What it holds is plain state, worked on by
//! the overlay's actor as the link shows it the chat and as the owner types and clicks; what it answers is the chat to
//! tell the link, and what it shows is laid out by the overlay's screen.
//!
//! ```text
//! ShowTabs ──> the tabs: "console", a break, then each session not archived, a light divider between them, a mark on
//!              one working or waiting on the owner; a session made here opened once it lists
//! ShowEvents ──> the session's transcript, its rows built again;  a line of this engine's echoed ──> its own id
//! ShowAsk ──> kept until answered here, or its session waits on none
//! a tab chosen, by a press, by its place, or the next or the one before ──> Close the one open, Open the chosen from
//!              the last entry held
//! a line ──> Say, Run here then Ran, or run here alone (`prompt`)
//! the chat's words, the engine's tools `new`, `stop`, and `archive` ──> New, Interrupt, or Archive (`words`)
//! ```
//!
//! Until the hub lists the sessions, which it never does to an untrusted engine, the console is the engine's own, as it
//! was before the chat: no tabs show, and every line runs here. Which tab is open, where its transcript is scrolled,
//! and the line half typed are the engine's alone, and never sent.

mod prompt;
mod read;
#[cfg(test)]
mod tests;
mod transcript;

pub use prompt::{Typed, typed};
#[cfg(test)]
use transcript::Ink;
pub use transcript::{Look, Mine, Row};

use crate::colour::{BONE, CONSOLE_OPACITY, SLATE_LIGHTER, STONE};
use aethereal_data::Plain;
use aethereal_render::text::Colour;
use aethereal_sessions::{Chat, Entry, Numbered, ShowAsk, Speaker, State, Tab};
use aethereal_ui::markdown::Blocks;
use core::iter;
use std::collections::BTreeMap;
use transcript::{Compaction, Transcript};

/// The title of the console's own tab, where every line runs here.
pub const CONSOLE_TITLE: &str = "console";

/// The most lines sent and not yet echoed kept to learn the engine's id from: the oldest leave past it.
const MOST_UNECHOED: usize = 16;

/// The sessions the hub has listed.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum Listed {
    /// None: the hub has not listed them, as it never does to an untrusted engine.
    Unlisted,
    /// As the hub last listed them.
    Tabs(Vec<Tab>),
}

/// Which tab of the console is open.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The console's own: the engine's scrollback, every line run here.
    Console,
    /// The session numbered so.
    Session(u32),
}

/// A line this engine sent a session, `/` commands by their line, whose echo in the transcript tells it its own id.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
struct Sent {
    session: u32,
    text: String,
}

/// A tab as the console shows it: its place, the console's own first, its title, whether it is open, and what parts it
/// from the tab before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title {
    pub place: u32,
    pub name: String,
    pub open: bool,
    pub parting: Parting,
}

/// What parts a tab from the one before it: the console's own tab is the engine's, and each session's is Claude's, so
/// the first session's stands apart from it by a break, and the sessions' from each other by a light divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parting {
    /// Nothing: the first tab, the console's own.
    First,
    /// A break: the first session's tab, after the console's own.
    Break,
    /// A light divider: any later session's tab.
    Light,
}

impl Parting {
    /// What parts the tab at `place` from the one before it.
    const fn at(place: u32) -> Self {
        match place {
            0 => Self::First,
            1 => Self::Break,
            _ => Self::Light,
        }
    }

    /// Whether anything parts the tab from the one before it.
    pub const fn parts(self) -> bool {
        !matches!(self, Self::First)
    }
}

impl Title {
    /// Its colour: bone while open, and stone otherwise.
    pub fn colour(&self) -> Colour {
        let swatch = if self.open {
            BONE
        } else {
            STONE
        };

        swatch.opaque()
    }

    /// What it is drawn on: slate while open, and nothing otherwise.
    pub fn background(&self) -> Colour {
        if self.open {
            SLATE_LIGHTER.at(CONSOLE_OPACITY)
        } else {
            Colour::CLEAR
        }
    }
}

/// The chat as the console holds it.
#[derive(Plain, Debug, Clone)]
pub struct Chats {
    listed: Listed,
    open: Open,
    /// The transcript of each session whose tab was opened, kept when it is closed, so it opens again from where it
    /// stood.
    transcripts: BTreeMap<u32, Transcript>,
    mine: Mine,
    /// The lines sent whose echo has not come, oldest first, while the engine does not know its id.
    unechoed: Vec<Sent>,
    /// The asks that wait on the owner, in the order they came.
    asks: Vec<ShowAsk>,
    making: Making,
}

/// A session the owner made here, which the console opens once the hub lists it.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
enum Making {
    /// None waits to be listed.
    Nothing,
    /// The session named `name`, made when the hub listed the sessions `listed`: the first session so named the hub
    /// lists that is not among them is it.
    Named { name: String, listed: Vec<u32> },
}

/// Which session one of the chat's words names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Which {
    /// The one whose tab is open.
    Open,
    /// The one listed under this name.
    Named(String),
}

impl Default for Chats {
    fn default() -> Self {
        Self {
            listed: Listed::Unlisted,
            open: Open::Console,
            transcripts: BTreeMap::new(),
            mine: Mine::Unknown,
            unechoed: Vec::new(),
            asks: Vec::new(),
            making: Making::Nothing,
        }
    }
}

impl Chats {
    /// Whether the hub has listed the sessions, so the console shows the chat.
    pub const fn is_listed(&self) -> bool {
        matches!(self.listed, Listed::Tabs(_))
    }

    /// Which tab is open.
    pub const fn open(&self) -> Open {
        self.open
    }

    /// The sessions the hub listed, as tabs: those not archived, in order.
    fn live(&self) -> Vec<&Tab> {
        let Listed::Tabs(tabs) = &self.listed else {
            return Vec::new();
        };

        tabs.iter().filter(|tab| tab.state != State::Archived).collect()
    }

    /// The sessions as the hub now lists them. The first listing opens the newest session not archived; a session
    /// whose tab is open and that is archived or gone leaves the console's own tab open. An ask of a session that
    /// waits on none is let go, answered at another engine.
    pub fn listed(&mut self, tabs: Vec<Tab>) -> Vec<Chat> {
        let first = !self.is_listed();
        self.listed = Listed::Tabs(tabs);
        let live: Vec<u32> = self.live().iter().map(|tab| tab.session).collect();
        let asking: Vec<u32> = self.live().iter().filter(|tab| tab.asking > 0).map(|tab| tab.session).collect();
        self.asks.retain(|ask| asking.contains(&ask.session));

        let mut chat = match self.open {
            Open::Session(session) if !live.contains(&session) => vec![Chat::Close { session }],
            Open::Session(_) | Open::Console => Vec::new(),
        };
        let gone = !chat.is_empty();
        if gone {
            self.open = Open::Console;
        }
        let newest = live.last().copied();
        let made = self.made();
        if let Some(session) = made {
            self.making = Making::Nothing;
            chat.extend(self.choose_open(Open::Session(session)));
        } else if let (true, Some(session)) = (first, newest) {
            chat.extend(self.choose_open(Open::Session(session)));
        }

        chat
    }

    /// The session made here that the hub now lists for the first time: none while none waits, or it is not yet
    /// listed.
    fn made(&self) -> Option<u32> {
        let Making::Named { name, listed } = &self.making else {
            return None;
        };

        self.live()
            .iter()
            .filter(|tab| tab.name == *name)
            .map(|tab| tab.session)
            .find(|session| !listed.contains(session))
    }

    /// Events of `session`'s transcript the hub showed: held, a line this engine sent and now sees echoed telling it
    /// its id, and the rows built again.
    pub fn shown(&mut self, session: u32, events: &[Numbered]) {
        let learned = self.learn(session, events);
        self.transcripts.entry(session).or_default().take(events);
        let mine = self.mine;
        if learned {
            for transcript in self.transcripts.values_mut() {
                transcript.build(mine);
            }
        } else if let Some(transcript) = self.transcripts.get_mut(&session) {
            transcript.build(mine);
        }
    }

    /// Learns the engine's id from `events` of `session`: the first line said or run at an engine whose text is the
    /// oldest this engine sent that session and has not seen echoed is this engine's. Answers whether it learned it.
    fn learn(&mut self, session: u32, events: &[Numbered]) -> bool {
        if let Mine::Engine(_) = self.mine {
            return false;
        }
        for Numbered { entry, .. } in events {
            let (engine, text) = match entry {
                Entry::Said { by: Speaker::Engine { engine }, text } => (*engine, text),
                Entry::Ran { engine, line, .. } => (*engine, line),
                _ => continue,
            };
            let oldest = self.unechoed.iter().find(|sent| sent.session == session);
            let echoed = oldest.is_some_and(|sent| sent.text == *text);
            if echoed {
                self.mine = Mine::Engine(engine);
                self.unechoed.clear();
                return true;
            }
        }

        false
    }

    /// Keeps `text`, sent to `session`, to learn the engine's id from its echo, while it does not know it.
    fn sent(&mut self, session: u32, text: &str) {
        if let Mine::Engine(_) = self.mine {
            return;
        }
        self.unechoed.push(Sent { session, text: String::from(text) });
        let over = self.unechoed.len().saturating_sub(MOST_UNECHOED);
        self.unechoed.drain(..over);
    }

    /// An ask the hub showed, kept until it is answered: one held already is passed over.
    pub fn asked(&mut self, ask: ShowAsk) {
        let held = self.asks.iter().any(|held| held.ask == ask.ask);
        if !held {
            self.asks.push(ask);
        }
    }

    /// The tab at `index` chosen, the console's own first: the tab open is closed, and the chosen session opened from
    /// the last entry held of it.
    pub fn choose(&mut self, index: u32) -> Vec<Chat> {
        let chosen = match index.checked_sub(1) {
            None => Open::Console,
            Some(place) => {
                let live = self.live();
                let at = usize::try_from(place).unwrap_or(usize::MAX);
                let Some(tab) = live.get(at) else {
                    return Vec::new();
                };
                Open::Session(tab.session)
            }
        };

        self.choose_open(chosen)
    }

    /// The tab after the one open chosen, or with `back` the one before, round from the last to the first.
    pub fn step(&mut self, back: bool) -> Vec<Chat> {
        let count = u32::try_from(self.live().len()).unwrap_or(u32::MAX).saturating_add(1);
        let selected = self.selected();
        let place = if back {
            selected.checked_sub(1).unwrap_or(count - 1)
        } else {
            (selected + 1) % count
        };

        self.choose(place)
    }

    /// Opens `chosen`, closing the tab open: nothing when it is open already.
    fn choose_open(&mut self, chosen: Open) -> Vec<Chat> {
        if chosen == self.open {
            return Vec::new();
        }
        let mut chat = Vec::new();
        if let Open::Session(session) = self.open {
            chat.push(Chat::Close { session });
        }
        if let Open::Session(session) = chosen {
            let after = self.transcripts.get(&session).map_or(0, Transcript::last);
            chat.push(Chat::Open { session, after });
        }
        self.open = chosen;

        chat
    }

    /// A line said to the session whose tab is open: nothing with the console's own open.
    pub fn say(&mut self, text: String) -> Vec<Chat> {
        let Open::Session(session) = self.open else {
            return Vec::new();
        };
        self.sent(session, &text);

        vec![Chat::Say { session, text }]
    }

    /// A `/` command run here, `line` with the `/` taken off, and what it printed, noted in the session whose tab was
    /// open when it was typed.
    pub fn ran(&mut self, session: u32, line: String, printed: String) -> Chat {
        self.sent(session, &line);

        Chat::Ran { session, line, printed }
    }

    /// Stops the turn of the session whose tab is open: nothing with the console's own open.
    pub fn interrupt(&self) -> Vec<Chat> {
        match self.open {
            Open::Session(session) => vec![Chat::Interrupt { session }],
            Open::Console => Vec::new(),
        }
    }

    /// A session made, named `name`, opened once the hub lists it.
    pub fn make(&mut self, name: String) -> Chat {
        let listed = self.live().iter().map(|tab| tab.session).collect();
        self.making = Making::Named { name: name.clone(), listed };

        Chat::New { name }
    }

    /// Stops the turn of the session `which` names, or says why no session is named.
    pub fn stop(&self, which: &Which) -> Result<Chat, String> {
        self.session(which).map(|session| Chat::Interrupt { session })
    }

    /// Archives the session `which` names, or says why no session is named.
    pub fn archive(&self, which: &Which) -> Result<Chat, String> {
        self.session(which).map(|session| Chat::Archive { session })
    }

    /// The session `which` names among those listed and not archived, or why none is: no tab of a session is open, no
    /// session has the name, or several have it.
    fn session(&self, which: &Which) -> Result<u32, String> {
        let name = match (which, self.open) {
            (Which::Open, Open::Session(session)) => return Ok(session),
            (Which::Open, Open::Console) => return Err(String::from("no session's tab is open: name the session")),
            (Which::Named(name), _) => name,
        };
        let named: Vec<u32> = self.live().iter().filter(|tab| tab.name == *name).map(|tab| tab.session).collect();
        match named.as_slice() {
            [session] => Ok(*session),
            [] => Err(format!("no session is named `{name}`")),
            _ => Err(format!("{} sessions are named `{name}`: open its tab, and leave the name out", named.len())),
        }
    }

    /// The owner's answer to the ask numbered `ask`: let go here at once.
    pub fn answer(&mut self, ask: u64, allow: bool) -> Chat {
        self.asks.retain(|held| held.ask != ask);

        Chat::Answer { ask, allow }
    }

    /// Opens or closes the subagent's fold that the open session's row keyed `key` is: nothing for any other row.
    pub fn fold(&mut self, key: u64) {
        let Open::Session(session) = self.open else {
            return;
        };
        let mine = self.mine;
        if let Some(transcript) = self.transcripts.get_mut(&session) {
            transcript.fold(key);
            transcript.build(mine);
        }
    }

    /// Each tab, the console's own first, each session's titled with its name and its marks: `*` while it works,
    /// compacts included, and `?` while it waits on the owner.
    pub fn titles(&self) -> Vec<Title> {
        let sessions = self.live().into_iter().map(|tab| {
            let compacting = self.transcripts.get(&tab.session).map(Transcript::compaction);
            let working = is_working(tab.state) || compacting == Some(Compaction::Compacting);
            let asking = tab.asking > 0;
            let mut title = tab.name.clone();
            if working {
                title.push_str(" *");
            }
            if asking {
                title.push_str(" ?");
            }
            title
        });

        let selected = self.selected();

        iter::once(String::from(CONSOLE_TITLE))
            .chain(sessions)
            .zip(0..)
            .map(|(title, place)| Title { place, name: title, open: place == selected, parting: Parting::at(place) })
            .collect()
    }

    /// The place of the open tab among the titles.
    pub fn selected(&self) -> u32 {
        let Open::Session(session) = self.open else {
            return 0;
        };
        let place = self.live().iter().position(|tab| tab.session == session).map_or(0, |at| at + 1);

        u32::try_from(place).unwrap_or(0)
    }

    /// The rows of the open session's transcript: none with the console's own open.
    pub fn rows(&self) -> Vec<&Row> {
        let Open::Session(session) = self.open else {
            return Vec::new();
        };

        self.transcripts.get(&session).map_or_else(Vec::new, |transcript| transcript.rows().iter().collect())
    }

    /// The blocks of the reply `row` of the open session's transcript is: none for any other row.
    pub fn reply(&self, row: &Row) -> Blocks {
        let Look::Reply { number, part } = row.look else {
            return Blocks::default();
        };
        let Open::Session(session) = self.open else {
            return Blocks::default();
        };

        self.transcripts.get(&session).map_or_else(Blocks::default, |transcript| transcript.reply(number, part))
    }

    /// The asks of the open session that wait on the owner.
    pub fn asks(&self) -> Vec<ShowAsk> {
        let Open::Session(session) = self.open else {
            return Vec::new();
        };

        self.asks.iter().filter(|ask| ask.session == session).cloned().collect()
    }

    /// How many sessions wait on the owner: what the band shows.
    pub fn waiting(&self) -> usize {
        self.live().iter().filter(|tab| tab.asking > 0).count()
    }
}

/// Whether a session in `state` is busy: making its container, running a turn or a background task, being told or
/// compacted as it winds down, or stopping.
const fn is_working(state: State) -> bool {
    match state {
        State::Starting | State::Working | State::Waiting | State::Noticed | State::Compacting | State::Stopping => {
            true
        }
        State::Idle | State::Stopped | State::Archived => false,
    }
}
