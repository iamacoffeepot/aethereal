//! `send_input`'s tests: the inputs asked reach the window's subscribers in order, before the answer; inputs that
//! cannot be performed are refused, and none of them is sent; and a window with no actor is answered as not open.

use super::*;
use crate::event::{Button, Event, Mode, Subscribe, Surface};
use crate::host::Opening;
use crate::perform::{MOST_INPUTS, MOST_NAME_BYTES};
use crate::windows::{Heard, Windowing, Windows};
use aethereal_actor::{Tells, Top};
use aethereal_data::{ActorPath, Kind, PathSegment};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Unheard};
use std::num::NonZeroUsize;

/// The surface the window is opened at.
const SURFACE: Surface = Surface { width: 640, height: 480, scale: 1.0 };

/// An actor that subscribes to the first window as it begins. The events it is sent are read from the turns the
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
    type Handles = (Event,);
    type BeginSends = (Tells<Path<Window>, Subscribe>,);
    type EndSends = ();

    fn begin(_: InputToolConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(WindowId::nth(0))).send(&Subscribe)?;

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
    let heard = harness
        .host()
        .engine()
        .take_turned()
        .iter()
        .filter(|turn| turn.path == subscriber)
        .filter_map(|turn| turn.read().ok())
        .collect();

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
