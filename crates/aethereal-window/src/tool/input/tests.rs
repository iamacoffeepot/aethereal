//! `send_input`'s tests: the inputs asked reach the window's subscriber in order, before the answer, the keys and text
//! through the keys it declares; inputs that cannot be performed are refused, and none of them is sent; a window with
//! no actor is answered as not open; and text is performed as typed, unless no host would send it.

use super::*;
use crate::event::{Button, Event, Mode, Subscribe, Surface};
use crate::host::Opening;
use crate::keys::{KeyEvent, KeySet, Keyed, KeysEntry, KeysListed};
use crate::perform::{MOST_INPUTS, MOST_NAME_BYTES, MOST_TEXT_BYTES};
use crate::pointing::{Probed, SubscribeProbes};
use crate::windows::{Heard, Windowing, Windows};
use aethereal_actor::{Tells, Top};
use aethereal_data::{ActorPath, Kind, PathSegment};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Turned, Unheard};
use aethereal_tools::{CommandLine, FlagsFault};
use std::num::NonZeroUsize;

/// The surface the window is opened at.
const SURFACE: Surface = Surface { width: 640, height: 480, scale: 1.0 };

/// An actor that subscribes to the first window as it begins, and stands where the renderer would, subscribed to its
/// probes, to list itself as hearing every key there. The events and keys it is sent are read from the turns the
/// engine recorded.
#[derive(Bound)]
#[kind(name = "aether.test.subscriber")]
struct Subscriber;

impl ChildOf for Subscriber {
    type Parent = Top;
}

impl Actor for Subscriber {
    type Config = InputToolConfig;
    type Key = ();
    type Handles = (Event, Keyed, Probed);
    type BeginSends =
        (Tells<Path<Window>, Subscribe>, Tells<Path<Window>, SubscribeProbes>, Tells<Path<Window>, KeysListed>);
    type EndSends = ();

    fn begin(_: InputToolConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let hears = KeysEntry { actor: turn.me().protocol(), entry: 0, keys: KeySet::All, blocks: true };
        let window = path::<Window>(WindowId::nth(0));
        turn.to(window).send(&Subscribe)?.send(&SubscribeProbes)?.send(&KeysListed { entries: vec![hears] })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Subscriber {
    type Sends = ();

    fn handle(&mut self, _: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Keyed> for Subscriber {
    type Sends = ();

    fn handle(&mut self, _: Keyed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Probed> for Subscriber {
    type Sends = ();

    fn handle(&mut self, _: Probed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What `turned` handed the subscriber, as the window heard it: an event, or a key or text its entry was sent; none
/// for a probe.
fn heard(turned: &Turned) -> Option<Event> {
    if let Ok(event) = turned.read() {
        return Some(event);
    }
    let Keyed { event, .. } = turned.read().ok()?;
    let event = match event {
        KeyEvent::KeyDown { code, key, repeat } => Event::KeyDown { code, key, repeat },
        KeyEvent::KeyUp { code, key } => Event::KeyUp { code, key },
        KeyEvent::Text { text } => Event::Text { text },
    };

    Some(event)
}

/// A harness over an engine with one window open, its subscriber, and the tool, recording its turns.
fn opened() -> Result<Harness<Bench>, Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let _feed = opening.open("window", SURFACE, Mode::Windowed, false);
    let (config, _hosting) = opening.config();
    let engine = setup
        .add_root::<Windows>(&(), config)?
        .add_root::<Subscriber>(&(), InputToolConfig)?
        .add_root::<InputTool>(&(), InputToolConfig)?
        .ready();
    let mut harness = Harness::bare(engine)?;
    harness.host().engine().record_turns();

    Ok(harness)
}

/// Calls `send_input` with `inputs` on `window`, and answers what it answered, with the events the subscriber was
/// sent meanwhile, in order.
fn send(harness: &mut Harness<Bench>, window: u64, inputs: Vec<Input>) -> Result<(Performed, Vec<Event>), Unheard> {
    let input = harness.put(to_bytes(&SendInput { window: WindowId::nth(window), inputs })?)?;
    let tool = InputTool::worker(harness.root::<InputTool>());
    let asked = harness.ask(tool, &Call { tool: String::from("send_input"), input })?;
    let Answered::Yes(Called::Output(output)) = harness.answer(asked)? else {
        panic!("send_input answers a value");
    };
    let Some(bytes) = harness.read(&output) else {
        panic!("the answer's bytes are held");
    };
    let performed = from_bytes::<Performed>(bytes, &mut Budget::new(INPUT_BYTES))?;
    harness.settle();
    let subscriber = ActorPath::from_root(&PathSegment::new(Subscriber::NAME, &()));
    let heard =
        harness.host().engine().take_turned().iter().filter(|turn| turn.path == subscriber).filter_map(heard).collect();

    Ok((performed, heard))
}

/// The key `W`, down.
fn w_down() -> Input {
    Input::KeyDown { code: String::from("KeyW"), key: String::from("w"), repeat: false }
}

#[test]
fn inputs_reach_the_window_s_subscribers_in_order_unless_one_cannot_be_performed() -> Result<(), Unheard> {
    // Catches inputs sent out of order, or answered before the last is sent on, a key sent to an unfocused window
    // dropped, a refused request that sent the inputs before the bad one, a non-finite position a host never sends
    // passed on, a name too long to be read within the engine's default limit, the most inputs not bounded, and a
    // window with no actor left unanswered.
    let mut harness = opened()?;
    let (_, stood) = send(&mut harness, 0, Vec::new())?;
    assert_eq!(stood, [Event::Resized(SURFACE), Event::Moded(Mode::Windowed), Event::FocusLost]);

    let click = vec![
        Input::PointerMoved { x: 10.0, y: 20.0 },
        Input::PointerDown { x: 10.0, y: 20.0, button: Button::Main },
        Input::PointerUp { x: 10.0, y: 20.0, button: Button::Main },
        w_down(),
    ];
    let (performed, heard) = send(&mut harness, 0, click.clone())?;
    assert_eq!(performed, Performed::Performed);
    assert_eq!(heard, click.into_iter().map(Event::from).collect::<Vec<_>>());

    let unfinite = vec![w_down(), Input::Wheel { x: 0.0, y: 0.0, delta_x: 0.0, delta_y: f32::NAN }];
    assert_eq!(send(&mut harness, 0, unfinite)?, (Performed::Refused(Unperformed::NotFinite { at: 1 }), Vec::new()));
    let long = Input::KeyUp { code: "K".repeat(MOST_NAME_BYTES as usize + 1), key: String::from("k") };
    let refused = Performed::Refused(Unperformed::NameTooLong { at: 1, most: MOST_NAME_BYTES });
    assert_eq!(send(&mut harness, 0, vec![w_down(), long])?, (refused, Vec::new()));
    let too_many = vec![Input::PointerMoved { x: 0.0, y: 0.0 }; MOST_INPUTS as usize + 1];
    let refused = Performed::Refused(Unperformed::TooMany { most: MOST_INPUTS });
    assert_eq!(send(&mut harness, 0, too_many)?, (refused, Vec::new()));

    assert_eq!(send(&mut harness, 7, vec![w_down()])?, (Performed::Refused(Unperformed::NotOpen), Vec::new()));

    Ok(())
}

#[test]
fn text_reaches_the_window_s_subscribers_unless_it_is_too_long_empty_or_holds_a_control_character()
-> Result<(), Unheard> {
    // Catches text performed as another event, or dropped; text at the most refused, or text past it, which an engine
    // reads only when set to read more, passed on; and text that is empty or holds a control character, which no host
    // sends as text, passed on, so a text field inserts the Enter or the Backspace a key already said; and the input a
    // refusal names counted from the wrong place.
    let mut harness = opened()?;
    let _ = send(&mut harness, 0, Vec::new())?;

    let most = "a".repeat(MOST_TEXT_BYTES as usize);
    let typed = vec![w_down(), Input::Text { text: String::from("w日本") }, Input::Text { text: most.clone() }];
    let (performed, heard) = send(&mut harness, 0, typed)?;
    assert_eq!(performed, Performed::Performed);
    let expected = [
        Event::KeyDown { code: String::from("KeyW"), key: String::from("w"), repeat: false },
        Event::Text { text: String::from("w日本") },
        Event::Text { text: most },
    ];
    assert_eq!(heard, expected);

    let long = Input::Text { text: "a".repeat(MOST_TEXT_BYTES as usize + 1) };
    let refused = Performed::Refused(Unperformed::TextTooLong { at: 1, most: MOST_TEXT_BYTES });
    assert_eq!(send(&mut harness, 0, vec![w_down(), long])?, (refused, Vec::new()));
    for text in ["", "\r", "ab\u{8}"] {
        let not_text = vec![w_down(), Input::Text { text: String::from(text) }];
        let refused = Performed::Refused(Unperformed::NotText { at: 1 });
        assert_eq!(send(&mut harness, 0, not_text)?, (refused, Vec::new()), "{text:?}");
    }

    Ok(())
}

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = InputTool::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}
