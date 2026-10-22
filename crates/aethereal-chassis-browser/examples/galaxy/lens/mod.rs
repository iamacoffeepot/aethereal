//! The lens: the root that shows the galaxy. At `Start` it makes what it draws one ask after another, each answer
//! carrying what was made so far: the quad, the stars' program, the bloom's effects, the uniforms each frame's stars
//! are drawn with, the glow's frame, and the window's frame. It asks the disc to move the stars in the glow's frame's
//! compute pass, and once the disc answers that they move it places them in both frames.

mod bloom;
mod frames;
mod stars;

use crate::camera::Camera;
use crate::disc::Disc;
use crate::mail::{Canvas, Move, Moving, Nothing};
use crate::page::say;
use aethereal_actor::{
    Actor, Allows, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Start, Top,
    Turn, path,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::effect::{CreateEffect, EffectId, Effected};
use aethereal_render::frame::builder::Float;
use aethereal_render::frame::{CreateFrame, Frame, FrameId, FrameRefused, Framed, ShapeOf, Shaped};
use aethereal_render::mesh::{CreateMesh, MeshId, Meshed};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Place, Placed};
use aethereal_render::program::{CreateProgram, ProgramId, Programmed};
use aethereal_render::shape::ColourFormat;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId};
use bloom::Effects;
use stars::Drawing;

/// The lens: how far it has come in showing the galaxy on the canvas.
#[derive(Bound)]
pub enum Lens {
    /// Its asks are under way.
    Making { canvas: Canvas },
    /// Its frames are made, and it waits for the stars to move to place them, drawn by `drawing` in the glow's frame
    /// `glow` and the canvas's window.
    Framed { canvas: Canvas, drawing: Drawing, glow: FrameId },
    /// The stars are placed.
    Showing,
}

impl Kind for Lens {
    const NAME: KindName = KindName::new("aether.example.galaxy.lens");
}

impl ChildOf for Lens {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the composite draws into.
#[derive(Plain)]
pub struct Learned {
    window: ColourFormat,
}

impl Kind for Learned {
    const NAME: KindName = KindName::new("aether.example.galaxy.learned");
}

/// What was made before the stars' program was asked for.
#[derive(Plain)]
pub struct MadeMesh {
    window: ColourFormat,
    mesh: MeshId,
}

impl Kind for MadeMesh {
    const NAME: KindName = KindName::new("aether.example.galaxy.made_mesh");
}

/// What was made before the bloom's effects were asked for.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Drawn {
    window: ColourFormat,
    mesh: MeshId,
    program: ProgramId,
}

/// Which of the bloom's effects was asked for, with those made before it.
#[derive(Debug, Clone, Copy, Plain)]
pub enum Effecting {
    Bright(Drawn),
    Across(Drawn, EffectId<(Float,)>),
    Down(Drawn, EffectId<(Float,)>, EffectId<(Float,)>),
    Composite(Drawn, EffectId<(Float,)>, EffectId<(Float,)>, EffectId<(Float,)>),
}

impl Kind for Effecting {
    const NAME: KindName = KindName::new("aether.example.galaxy.effecting");
}

/// What was made before the uniforms were asked for.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Made {
    drawn: Drawn,
    effects: Effects,
}

/// Which uniforms were asked for: the glow's stars', then the window's, with the glow's.
#[derive(Plain)]
pub enum Viewing {
    Glow(Made),
    Shown(Made, UniformsId),
}

impl Kind for Viewing {
    const NAME: KindName = KindName::new("aether.example.galaxy.viewing");
}

/// Which frame was asked for: the glow's, with all that draws the stars, then the window's, with the glow's.
#[derive(Plain)]
pub enum Framing {
    Glow(Effects, Drawing),
    Shown(Drawing, FrameId),
}

impl Kind for Framing {
    const NAME: KindName = KindName::new("aether.example.galaxy.framing");
}

impl Actor for Lens {
    type Config = Canvas;
    type Key = ();
    type Handles = (
        On<Start>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Effected, Effecting>,
        Answer<Uniformed, Viewing>,
        Answer<Framed, Framing>,
        Answer<Moving, Nothing>,
        Answer<Placed, Nothing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(canvas: Canvas, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self::Making { canvas })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Lens {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { canvas } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(canvas.window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Learned>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            say(format!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let (layout, corners, indices) = stars::quad();
        let (vertices, indices) = match (turn.put(corners), turn.put(indices)) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("no quad asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(()))
            .ask(&CreateMesh { layout, vertices, indices }, &Learned { window: shape.colour })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Learned>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateProgram, MadeMesh>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(stars::STARS_WGSL)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no stars' program asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&stars::program(wgsl), &MadeMesh { window: learned.window, mesh })?;

        Ok(())
    }
}

impl Effecting {
    /// The WGSL of the effect asked for so.
    const fn source(&self) -> &'static str {
        match self {
            Self::Bright(_) => bloom::BRIGHT_WGSL,
            Self::Across(..) | Self::Down(..) => bloom::BLUR_WGSL,
            Self::Composite(..) => bloom::COMPOSITE_WGSL,
        }
    }

    /// The ask for the effect asked for so, of the WGSL the blob `wgsl` holds.
    fn create(&self, wgsl: BlobRef) -> CreateEffect {
        match self {
            Self::Bright(_) => bloom::bright(wgsl),
            Self::Across(..) => bloom::across(wgsl),
            Self::Down(..) => bloom::down(wgsl),
            Self::Composite(drawn, ..) => bloom::composite(wgsl, drawn.window),
        }
    }

    /// What follows the effect asked for so once it is made, `made`: the next effect, or, once the last is, all that
    /// was made.
    const fn then(self, made: EffectId<(Float,)>) -> Next {
        match self {
            Self::Bright(drawn) => Next::Effect(Self::Across(drawn, made)),
            Self::Across(drawn, bright) => Next::Effect(Self::Down(drawn, bright, made)),
            Self::Down(drawn, bright, across) => Next::Effect(Self::Composite(drawn, bright, across, made)),
            Self::Composite(drawn, bright, across, down) => {
                Next::Made(Made { drawn, effects: Effects { bright, across, down, composite: made } })
            }
        }
    }
}

/// What follows an effect once it is made.
enum Next {
    /// The next effect, asked for so.
    Effect(Effecting),
    /// Nothing more of the bloom: all that was made.
    Made(Made),
}

/// Asks for the effect `effecting` names, in a turn whose sends allow it, saying why it was not when its WGSL could
/// not be put.
fn ask_effect<S, I>(effecting: Effecting, turn: &mut Turn<'_, Lens, S>) -> Result<(), Fault>
where
    S: Allows<Asks<Path<Renderer>, CreateEffect, Effecting>, I>,
{
    let wgsl = match turn.put(Vec::from(effecting.source())) {
        Ok(wgsl) => wgsl,
        Err(why) => {
            say(format!("no effect asked: {why}"));
            return Ok(());
        }
    };
    turn.to(path::<Renderer>(())).ask(&effecting.create(wgsl), &effecting)?;

    Ok(())
}

impl Handles<Answer<Programmed, MadeMesh>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Effecting>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Programmed>, MadeMesh),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            say(format!("the stars' program was not made: {answered:?}"));
            return Ok(());
        };

        ask_effect(Effecting::Bright(Drawn { window: made.window, mesh: made.mesh, program }), turn)
    }
}

impl Handles<Answer<Effected, Effecting>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Effecting>, Asks<Path<Renderer>, CreateUniforms, Viewing>);

    fn handle(
        &mut self,
        (answered, effecting): (Answered<Effected>, Effecting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(made)) = answered else {
            say(format!("an effect was not made: {answered:?}"));
            return Ok(());
        };
        let Some(effect) = EffectId::of(&made) else {
            say(format!("an effect reads what it was not asked to: {:?}", made.reads()));
            return Ok(());
        };
        let Self::Making { canvas } = *self else {
            return Ok(());
        };
        let made = match effecting.then(effect) {
            Next::Effect(effecting) => return ask_effect(effecting, turn),
            Next::Made(made) => made,
        };
        let [width, height] = [canvas.width, canvas.height];
        let glow = frames::glow_size(width, height);
        let Ok(bytes) = turn.put(stars::glow_view(Camera::of(glow.width, glow.height))) else {
            say(String::from("no glow's view asked: its bytes were not put"));
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &Viewing::Glow(made))?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, Viewing>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, Viewing>, Asks<Path<Renderer>, CreateFrame, Framing>);

    fn handle(
        &mut self,
        (answered, viewing): (Answered<Uniformed>, Viewing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(view)) = answered else {
            say(format!("a view was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { canvas } = *self else {
            return Ok(());
        };
        let (made, glow) = match viewing {
            Viewing::Glow(made) => {
                let Ok(bytes) = turn.put(stars::shown_view(Camera::of(canvas.width, canvas.height))) else {
                    say(String::from("no window's view asked: its bytes were not put"));
                    return Ok(());
                };
                turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &Viewing::Shown(made, view))?;
                return Ok(());
            }
            Viewing::Shown(made, glow) => (made, glow),
        };
        let drawing = Drawing { mesh: made.drawn.mesh, program: made.drawn.program, glow, shown: view };
        let create = match frames::glow(frames::glow_size(canvas.width, canvas.height), &made.effects) {
            Ok(create) => create,
            Err(why) => {
                say(format!("the glow's frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &Framing::Glow(made.effects, drawing))?;

        Ok(())
    }
}

impl Handles<Answer<Framed, Framing>> for Lens {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Framing>, Asks<Path<Disc>, Move, Nothing>);

    fn handle(
        &mut self,
        (answered, framing): (Answered<Framed>, Framing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Self::Making { canvas } = *self else {
            return Ok(());
        };
        match (answered, framing) {
            (Answered::Yes(Framed::Texture { frame, texture }), Framing::Glow(effects, drawing)) => {
                let create: Result<CreateFrame, FrameRefused> = frames::shown(canvas.window, &effects, texture);
                let create = match create {
                    Ok(create) => create,
                    Err(why) => {
                        say(format!("the window's frame was not built: {why:?}"));
                        return Ok(());
                    }
                };
                turn.to(path::<Renderer>(())).ask(&create, &Framing::Shown(drawing, frame))?;
            }
            (Answered::Yes(Framed::Made(_)), Framing::Shown(drawing, glow)) => {
                turn.to(path::<Disc>(())).ask(&Move { frame: glow }, &Nothing)?;
                *self = Self::Framed { canvas, drawing, glow };
            }
            (answered, _) => say(format!("a frame was not made: {answered:?}")),
        }

        Ok(())
    }
}

impl Handles<Answer<Moving, Nothing>> for Lens {
    type Sends = (Asks<Path<Renderer>, Place, Nothing>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Moving>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(moving) = answered else {
            say(format!("the stars do not move: {answered:?}"));
            return Ok(());
        };
        let Self::Framed { canvas, drawing, glow } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&drawing.place(moving.stars, glow, canvas.window), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Nothing>> for Lens {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Placed>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("the stars were not placed: {answered:?}"));
            return Ok(());
        };
        *self = Self::Showing;

        Ok(())
    }
}
