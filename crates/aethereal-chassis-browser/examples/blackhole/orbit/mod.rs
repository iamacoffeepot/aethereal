//! The orbit: the root that keeps the camera. Asked for the view, it holds the ask while it makes the uniforms the
//! photons are traced from, then answers with them. It follows the window's pointer, which drags the camera round the
//! hole, and at each `Tick` it turns the disk's time on, drifts the camera when nothing drags it, and writes the view.

mod pointer;
mod view;

use crate::mail::{Canvas, Look, Looking, Nothing};
use crate::page::say;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Level, On, Path, Tells,
    Tick, ToSender, Top, Turn, path,
};
use aethereal_data::{Bound, Kind, KindName};
use aethereal_render::Renderer;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_window::{self as window, Event, Window};
use core::mem;
use core::time::Duration;
use pointer::Pointer;
use view::View;

/// The longest step the camera and the disk are moved by, in nanoseconds: a frame that took longer, as when the page
/// was hidden, moves them no further, so nothing jumps.
const LONGEST_STEP_NANOS: u64 = 250_000_000;

/// The orbit: the window it follows, the pointer as it last heard it, the camera's view, and how far it has come in
/// handing the view to the GPU.
#[derive(Bound)]
pub struct Orbit {
    canvas: Canvas,
    pointer: Pointer,
    view: View,
    run: Run,
}

impl Kind for Orbit {
    const NAME: KindName = KindName::new("aether.example.blackhole.orbit");
}

impl Carries<Look> for Orbit {
    type Answers = (Looking,);
}

impl ChildOf for Orbit {
    type Parent = Top;
}

/// How far the orbit has come in handing the view to the GPU.
#[derive(Bound)]
pub enum Run {
    /// It waits to be asked for the view.
    Waiting,
    /// Its uniforms are asked for, and it holds the ask for the view, to answer once they are made.
    Making { asked: Held<Looking> },
    /// It writes the view into `uniforms` each frame.
    Running { uniforms: UniformsId },
}

impl Actor for Orbit {
    type Config = Canvas;
    type Key = ();
    type Handles = (On<Tick>, Event, Look, Answer<Uniformed, Nothing>);
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(canvas: Canvas, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(canvas.window)).send(&window::Subscribe)?;
        turn.subscribe::<Tick>();

        Ok(Self { canvas, pointer: Pointer::Up, view: View::begun(), run: Run::Waiting })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The pixels of a side of a surface, as a float: a canvas's side is far below what sixteen bits count.
fn pixels(side: u32) -> f32 {
    f32::from(u16::try_from(side).unwrap_or(u16::MAX).max(1))
}

impl Orbit {
    /// The bytes of the uniforms of the view as it now is, on the window as it now is.
    fn uniforms(&self) -> Vec<u8> {
        self.view.uniforms(pixels(self.canvas.width) / pixels(self.canvas.height))
    }
}

impl Handles<Event> for Orbit {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        if let Event::Resized(surface) = event {
            self.canvas = Canvas { width: surface.width, height: surface.height, ..self.canvas };
        }
        let (pointer, dragged) = self.pointer.heard(&event);
        self.view = self.view.dragged(dragged, pixels(self.canvas.height));
        self.pointer = pointer;

        Ok(())
    }
}

impl Handles<Look> for Orbit {
    type Sends = (ToSender<Looking>, Asks<Path<Renderer>, CreateUniforms, Nothing>);

    fn handle(&mut self, _: Look, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Run::Waiting = self.run else {
            return Ok(());
        };
        let bytes = match turn.put(self.uniforms()) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no view asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &Nothing)?;
        self.run = Run::Making { asked: turn.hold()? };

        Ok(())
    }
}

impl Handles<Answer<Uniformed, Nothing>> for Orbit {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Uniformed>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            say(format!("the view was not made: {answered:?}"));
            return Ok(());
        };
        let Run::Making { asked } = mem::replace(&mut self.run, Run::Running { uniforms }) else {
            return Ok(());
        };
        turn.answer(asked, &Looking { view: uniforms })?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Orbit {
    type Sends = (Tells<Path<Renderer>, WriteUniforms>,);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let seconds = Duration::from_nanos(tick.delta_nanos.min(LONGEST_STEP_NANOS)).as_secs_f32();
        self.view = self.view.later(seconds, self.pointer.held());
        let Run::Running { uniforms } = self.run else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(self.uniforms()) else {
            turn.log(Level::Warn, "the view was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms, first: 0, bytes })?;

        Ok(())
    }
}
