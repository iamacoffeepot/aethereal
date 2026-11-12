//! The keys' cascade (ADR-0055): a window's keys delivered top-down through the keys its actors declared in their draw
//! lists, in the declared order of the draw calls, until one blocks.
//!
//! An actor declares the keys it hears in a window with a `Keys` entry in its draw list, at its place in the sequence.
//! The renderer, the subscriber of the window's probes, sends the window those entries, the last called first,
//! [`KeysListed`], whenever they change. The window sends each key that goes down, as [`Keyed`], to the actor of each
//! entry whose [`KeySet`] holds it, from the top, until the first that blocks: so focus is a declaration, and a screen
//! that takes the keys lists `KeySet::All` with `blocks: true` while it wants them.
//!
//! - **A key that is down keeps whom its press was sent to:** its repeats and its release go there, however the
//!   entries changed since. The window losing the keyboard forgets every key down, as their releases never come.
//! - **Typed text follows the `KeyDown` that typed it,** to wherever the latest key that went down was sent, as a host
//!   sends a key's text after it (ADR-0051).
//!
//! Keys, typed text, and repeats are not sent to a window's raw subscribers: one would hear keys a declaration above
//! it blocked, and blocking would then mean nothing.

#[cfg(test)]
mod tests;

use crate::event::Event;
use crate::window::WindowId;
use aethereal_actor::ProtocolRef;
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// The keys an entry hears, each by its place on the keyboard, its `code`, as the W3C UI Events name it (`"KeyW"`,
/// `"Backquote"`), whatever the layout.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.key_set")]
pub enum KeySet {
    /// Every key.
    All,
    /// These keys.
    Of(Vec<String>),
}

impl KeySet {
    /// The keys at `codes`.
    #[must_use]
    pub fn of<const N: usize>(codes: [&str; N]) -> Self {
        Self::Of(codes.into_iter().map(String::from).collect())
    }

    /// Whether it holds the key at `code`.
    #[must_use]
    pub fn holds(&self, code: &str) -> bool {
        match self {
            Self::All => true,
            Self::Of(codes) => codes.iter().any(|held| held == code),
        }
    }
}

/// The `Keys` entries of the draw lists walked for a window's main frame, sent to the window's actor by the subscriber
/// of its probes whenever they change: the last called first. The window sends its keys through them from the next it
/// hears.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.keys_listed")]
pub struct KeysListed {
    pub entries: Vec<KeysEntry>,
}

/// One `Keys` entry: the entry numbered `entry`, from 0, of the draw list `actor` sent, the keys it hears, and whether
/// it blocks them from the entries beneath it.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.keys_entry")]
pub struct KeysEntry {
    pub actor: ProtocolRef<KeyHearer>,
    pub entry: u32,
    pub keys: KeySet,
    pub blocks: bool,
}

/// What an actor whose draw list declares keys handles.
pub type KeyHearer = (Keyed,);

/// A key or typed text, sent to an actor through the entry numbered `entry` of its draw list, which declared the keys
/// it hears in `window`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.keyed")]
pub struct Keyed {
    pub window: WindowId,
    pub entry: u32,
    pub event: KeyEvent,
}

/// What the keyboard did, named as [`Event`]'s keys and text are.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum KeyEvent {
    /// A key went down, or repeats as it is held, as [`Event::KeyDown`] says.
    KeyDown { code: String, key: String, repeat: bool },
    /// A key came up, as [`Event::KeyUp`] says.
    KeyUp { code: String, key: String },
    /// Text typed, as [`Event::Text`] says: sent where the key that typed it was.
    Text { text: String },
}

/// The most keys a window keeps a record of as down. Far more than a keyboard has, it bounds what performed keys that
/// never come up can hold (ADR-0046); a key pressed past it is sent through the entries as they stand, its release
/// too.
const MOST_KEYS_DOWN: usize = 256;

/// An entry a key is delivered to: an entry of an actor's draw list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
struct Reached {
    actor: ProtocolRef<KeyHearer>,
    entry: u32,
}

/// A window's keys, as its actor keeps them: the `Keys` entries as they were last listed, the last called first; each
/// key that is down, by its `code`, with the entries its press was sent to; and the entries the latest key that went
/// down was sent to, which typed text follows.
#[derive(Debug, Default, Bound)]
pub struct KeyCascade {
    listed: Vec<KeysEntry>,
    down: BTreeMap<String, Vec<Reached>>,
    typing: Vec<Reached>,
}

impl KeyCascade {
    /// Takes in the entries a frame lists, which every key from the next goes through. Keys down keep whom their presses
    /// were sent to.
    pub fn listed(&mut self, listed: KeysListed) {
        self.listed = listed.entries;
    }

    /// Takes in `event`, heard by `window`, and answers whom it is sent to, in order: a press goes through the entries
    /// and is recorded, unless its key is down already, when it goes as its record says, as a repeat does; a release
    /// goes as its press did, and its record is dropped; typed text goes where the latest press went. The window losing
    /// the keyboard forgets every key down. Any other event is not the keys'.
    pub fn heard(&mut self, window: WindowId, event: &Event) -> Vec<(ProtocolRef<KeyHearer>, Keyed)> {
        let (reached, event) = match event {
            Event::KeyDown { code, key, repeat } => {
                let reached = self.press(code);
                (reached, KeyEvent::KeyDown { code: code.clone(), key: key.clone(), repeat: *repeat })
            }
            Event::KeyUp { code, key } => {
                let reached = self.down.remove(code).unwrap_or_else(|| cascade(&self.listed, code));
                (reached, KeyEvent::KeyUp { code: code.clone(), key: key.clone() })
            }
            Event::Text { text } => (self.typing.clone(), KeyEvent::Text { text: text.clone() }),
            Event::FocusLost => {
                self.down.clear();
                return Vec::new();
            }
            Event::PointerMoved { .. }
            | Event::PointerDown { .. }
            | Event::PointerUp { .. }
            | Event::Wheel { .. }
            | Event::Resized(_)
            | Event::Moded(_)
            | Event::FocusGained
            | Event::CloseRequested
            | Event::Closed => return Vec::new(),
        };

        reached.into_iter().map(|to| (to.actor, Keyed { window, entry: to.entry, event: event.clone() })).collect()
    }

    /// The entries a press of the key at `code` is sent to: those its record says, when it is down already, or those it
    /// reaches through the entries, recorded unless the record is full. Typed text follows them from here.
    fn press(&mut self, code: &str) -> Vec<Reached> {
        let reached = if let Some(pressed) = self.down.get(code) {
            pressed.clone()
        } else {
            let reached = cascade(&self.listed, code);
            let is_full = self.down.len() >= MOST_KEYS_DOWN;
            if !is_full {
                self.down.insert(String::from(code), reached.clone());
            }
            reached
        };
        self.typing.clone_from(&reached);

        reached
    }
}

/// The entries the key at `code` reaches through `listed`, from its top: each whose set holds it, until the first of
/// those that blocks, which is reached too. An entry whose set does not hold it is passed over, whether it blocks or
/// not.
fn cascade(listed: &[KeysEntry], code: &str) -> Vec<Reached> {
    let mut reached = Vec::new();
    for listed in listed {
        if !listed.keys.holds(code) {
            continue;
        }
        reached.push(Reached { actor: listed.actor, entry: listed.entry });
        if listed.blocks {
            break;
        }
    }

    reached
}
