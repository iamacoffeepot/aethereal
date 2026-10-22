//! The page's entry points, and an actor that asks the renderer for meshes, a texture, and programs.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, On, Path, Refused, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::{Bound, Plain};
use aethereal_render::mesh::{Attribute, CreateMesh, Meshed, VertexFormat, VertexLayout};
use aethereal_render::program::{Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, Programmed};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::texture::{CreateTexture, TextureFormat, Textured};
use aethereal_render::{Release, Renderer};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::Windows;
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the maker was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the maker was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window,
/// the renderer, which opens WebGPU on it, and the maker.
///
/// # Errors
///
/// When the canvas or WebGPU on it will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let program =
        Browser::program().with_cycle::<(Tick, Render)>().root::<Windows>().root::<Renderer>().root::<Maker>();

    Ok(Program { started: program.run()?.await? })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
    }

    /// What the maker was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The maker needs no window and has no settings.
impl<H: Host> Bootstrap<H> for Maker {
    const SECTION: &'static str = "maker";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Nothing)
    }
}

/// An empty config.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// What the maker asked for, kept until it is answered.
#[derive(Plain)]
#[kind(name = "aether.example.asked")]
struct Asked {
    what: String,
}

/// An actor that, as the program starts, asks the renderer for a triangle, a texture of two by two pixels, a triangle
/// with an index past its last vertex, a program that shades the triangle, and one that reads a vertex the layout does
/// not give, says what each was answered, and releases the triangle once made.
#[derive(Bound)]
#[kind(name = "aether.example.maker")]
struct Maker;

impl ChildOf for Maker {
    type Parent = Top;
}

impl Actor for Maker {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Start>, Answer<Meshed, Asked>, Answer<Textured, Asked>, Answer<Programmed, Asked>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The bytes of `values`, as a blob is put.
fn bytes<const N: usize>(values: [u32; N]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_ne_bytes()).collect()
}

/// A program that places each corner where its two floats say, and shades it orange.
const SHADE: &str = "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.0, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(1.0, 0.5, 0.0, 1.0);
}
";

/// What the maker asks the renderer for: a triangle, a texture of two by two pixels, a triangle with an index past
/// its last vertex, each put as blobs through `turn`, a program that shades the triangle, and one that reads a colour
/// at a location its layout gives nothing at.
struct Requests {
    triangle: CreateMesh,
    texture: CreateTexture,
    past: CreateMesh,
    shade: CreateProgram,
    unread: CreateProgram,
}

impl Requests {
    /// The asks, their bytes put through `turn`: refused when a put is.
    fn put<S>(turn: &mut Turn<'_, Maker, S>) -> Result<Self, Refused> {
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let layout = VertexLayout { stride: 8, attributes: vec![position] };
        let corners = [0.0_f32, 0.5, -0.5, -0.5, 0.5, -0.5].map(f32::to_bits);

        let triangle = CreateMesh {
            layout: layout.clone(),
            vertices: turn.put(bytes(corners))?,
            indices: turn.put(bytes([0, 1, 2]))?,
        };
        let pixels = turn.put(vec![255; 16])?;
        let texture = CreateTexture { width: 2, height: 2, format: TextureFormat::Rgba8, pixels };
        let past = CreateMesh {
            layout: layout.clone(),
            vertices: turn.put(bytes(corners))?,
            indices: turn.put(bytes([0, 1, 3]))?,
        };
        let shade = CreateProgram {
            wgsl: turn.put(Vec::from(SHADE))?,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout,
            instances: PerInstance::None,
            bindings: Vec::new(),
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            // Made against a shape of its own, as it is drawn in no pass: a program is made for any shape.
            shape: Shape::with_depth(ColourFormat::Rgba8),
        };
        let tinted = SHADE.replace(
            "(@location(0) corner: vec2<f32>)",
            "(@location(0) corner: vec2<f32>, @location(1) tint: vec4<f32>)",
        );
        let unread = CreateProgram { wgsl: turn.put(tinted.into_bytes())?, ..shade.clone() };

        Ok(Self { triangle, texture, past, shade, unread })
    }
}

impl Handles<On<Start>> for Maker {
    type Sends = (
        Asks<Path<Renderer>, CreateMesh, Asked>,
        Asks<Path<Renderer>, CreateTexture, Asked>,
        Asks<Path<Renderer>, CreateProgram, Asked>,
    );

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let requests = match Requests::put(turn) {
            Ok(requests) => requests,
            Err(why) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };

        turn.to(path::<Renderer>(()))
            .ask(&requests.triangle, &Asked { what: String::from("a triangle") })?
            .ask(&requests.texture, &Asked { what: String::from("a texture") })?
            .ask(&requests.past, &Asked { what: String::from("an index past the end") })?
            .ask(&requests.shade, &Asked { what: String::from("a program") })?
            .ask(&requests.unread, &Asked { what: String::from("a program reading an unlaid vertex") })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Asked>> for Maker {
    type Sends = (Tells<Path<Renderer>, Release>,);

    fn handle(
        &mut self,
        (answered, asked): (Answered<Meshed>, Asked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Meshed::Made(mesh)) => {
                say(format!("{}: made {mesh:?}, then released", asked.what));
                turn.to(path::<Renderer>(())).send(&Release::Mesh(mesh))?;
            }
            Answered::Yes(Meshed::Refused(why)) => say(format!("{}: refused, {why:?}", asked.what)),
            Answered::No(why) => say(format!("{}: unanswered, {why:?}", asked.what)),
        }

        Ok(())
    }
}

impl Handles<Answer<Textured, Asked>> for Maker {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, asked): (Answered<Textured>, Asked),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Textured::Made(texture)) => say(format!("{}: made {texture:?}", asked.what)),
            Answered::Yes(Textured::Refused(why)) => say(format!("{}: refused, {why:?}", asked.what)),
            Answered::No(why) => say(format!("{}: unanswered, {why:?}", asked.what)),
        }

        Ok(())
    }
}

impl Handles<Answer<Programmed, Asked>> for Maker {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, asked): (Answered<Programmed>, Asked),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Programmed::Made(program)) => say(format!("{}: made {program:?}", asked.what)),
            Answered::Yes(Programmed::Refused(why)) => say(format!("{}: refused, {why:?}", asked.what)),
            Answered::No(why) => say(format!("{}: unanswered, {why:?}", asked.what)),
        }

        Ok(())
    }
}
