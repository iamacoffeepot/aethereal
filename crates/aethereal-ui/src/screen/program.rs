//! What a screen draws its quads with (ADR-0021, ADR-0023): the unit square, the library's program and its fill
//! program made against the shape of the screen's pass, the uniform of its frame's size, and a white pixel a solid quad
//! samples. Each is asked
//! for once the one before is made, the answer carrying what was made so far, from the shape the screen asks for as it
//! begins; and a font's metrics, asked for the first time a layout meets the font.

use super::orders::Orders;
use super::{Fresh, Screen, Viewport};
use crate::draw::{FILL_PROGRAM, PROGRAM, frame_bytes};
use aethereal_actor::{Answered, Bytes, Level};
use aethereal_data::Plain;
use aethereal_render::font::Metrics;
use aethereal_render::frame::Shaped;
use aethereal_render::mesh::{MeshId, Meshed};
use aethereal_render::program::{ProgramId, Programmed};
use aethereal_render::shape::Shape;
use aethereal_render::text::{Font, FontMetrics};
use aethereal_render::texture::{TextureId, Textured};
use aethereal_render::uniforms::{Uniformed, UniformsId};
use core::fmt::Debug;

/// The shape of the screen's pass was asked for, to make its program against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.shaping")]
pub struct Shaping;

/// The unit square was asked for, with the shape the program is made against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.meshing")]
pub struct Meshing {
    pub(super) shape: Shape,
}

/// One of the two programs was asked for, with the square and the shape they are made against: the library's, then
/// its fill program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.programming")]
pub struct Programming {
    pub(super) mesh: MeshId,
    pub(super) shape: Shape,
    pub(super) asked: Asked,
}

/// Which of a screen's programs was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Asked {
    /// The library's program, which draws every quad.
    Quads,
    /// The fill program, which draws plain fills, after the library's, `quads`, was made.
    Fills { quads: ProgramId },
}

/// The uniform of the frame's size was asked for, with the square and the programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.uniforming")]
pub struct Uniforming {
    pub(super) mesh: MeshId,
    pub(super) program: ProgramId,
    pub(super) fills: ProgramId,
}

/// The white pixel was asked for, with the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.whitening")]
pub struct Whitening {
    pub(super) mesh: MeshId,
    pub(super) program: ProgramId,
    pub(super) fills: ProgramId,
    pub(super) uniforms: UniformsId,
}

/// A font's metrics were asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.metrics_asked")]
pub struct MetricsAsked {
    pub(super) font: Font,
}

/// What a screen has made of what it draws quads with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Program {
    /// Its asks are under way, one after another.
    Making,
    /// Made: nothing is drawn before.
    Made(Made),
    /// A step was refused, and logged: the screen draws nothing.
    Unmade,
}

/// What a screen draws quads with, made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) struct Made {
    pub mesh: MeshId,
    pub program: ProgramId,
    pub fills: ProgramId,
    pub uniforms: UniformsId,
    pub white: TextureId,
}

impl Asked {
    /// The program's text.
    pub(super) const fn wgsl(self) -> &'static str {
        match self {
            Self::Quads => PROGRAM,
            Self::Fills { .. } => FILL_PROGRAM,
        }
    }
}

impl Program {
    /// What it made, once it is made.
    pub(super) const fn made(self) -> Option<Made> {
        match self {
            Self::Made(made) => Some(made),
            Self::Making | Self::Unmade => None,
        }
    }

    /// Writes the size of `viewport`'s frame to the uniform, once it is made: before, the uniform is made with the
    /// size as it is then.
    pub(super) fn write_frame(self, viewport: Viewport, orders: &mut Orders) {
        if let Self::Made(made) = self {
            orders.write_uniforms(made.uniforms, frame_bytes(viewport.frame_width, viewport.frame_height));
        }
    }
}

impl Screen {
    /// Leaves the program unmade, as `answered` says, and logs `what` was not made.
    fn unmade_program(&mut self, what: &str, answered: &dyn Debug, orders: &mut Orders) {
        self.program = Program::Unmade;
        orders.log(Level::Error, format!("the screen's {what} was not made, so it draws nothing: {answered:?}"));
    }

    /// The pass's shape came: the square is asked for.
    pub fn shaped(&mut self, answered: Answered<Shaped>, _: Shaping) -> Orders {
        let mut orders = Orders::default();
        match answered {
            Answered::Yes(Shaped::Shape(shape)) => orders.ask_mesh(Meshing { shape }),
            refused => self.unmade_program("program: its pass has no shape", &refused, &mut orders),
        }

        orders
    }

    /// The square was made: the library's program is asked for.
    pub fn meshed(&mut self, answered: Answered<Meshed>, meshing: Meshing) -> Orders {
        let mut orders = Orders::default();
        match answered {
            Answered::Yes(Meshed::Made(mesh)) => {
                orders.ask_program(Programming { mesh, shape: meshing.shape, asked: Asked::Quads });
            }
            refused => self.unmade_program("square", &refused, &mut orders),
        }

        orders
    }

    /// A program was made: after the library's, the fill program is asked for, against the shape the screen's pass
    /// answered; after the fill program, the uniform, holding the frame's size as it is now.
    pub fn programmed(&mut self, answered: Answered<Programmed>, programming: Programming) -> Orders {
        let mut orders = Orders::default();
        let Programming { mesh, shape, asked } = programming;
        let made = match answered {
            Answered::Yes(Programmed::Made(made)) => made,
            refused => {
                self.unmade_program("program", &refused, &mut orders);
                return orders;
            }
        };
        match asked {
            Asked::Quads => orders.ask_program(Programming { mesh, shape, asked: Asked::Fills { quads: made } }),
            Asked::Fills { quads } => {
                let viewport = self.viewport;
                let bytes = frame_bytes(viewport.frame_width, viewport.frame_height);
                orders.ask_uniforms(Uniforming { mesh, program: quads, fills: made }, bytes);
            }
        }

        orders
    }

    /// The uniform was made: the white pixel is asked for.
    pub fn uniformed(&mut self, answered: Answered<Uniformed>, uniforming: Uniforming) -> Orders {
        let mut orders = Orders::default();
        match answered {
            Answered::Yes(Uniformed::Made(uniforms)) => {
                let Uniforming { mesh, program, fills } = uniforming;
                orders.ask_white(Whitening { mesh, program, fills, uniforms });
            }
            refused => self.unmade_program("uniform", &refused, &mut orders),
        }

        orders
    }

    /// The white pixel was made, and with it all a screen draws quads with: the frame's size is written again, as it
    /// may have changed while they were made, and the next `Prepare` lays the screen out.
    pub fn whitened(&mut self, answered: Answered<Textured>, whitening: Whitening) -> Orders {
        let mut orders = Orders::default();
        match answered {
            Answered::Yes(Textured::Made(white)) => {
                let Whitening { mesh, program, fills, uniforms } = whitening;
                self.program = Program::Made(Made { mesh, program, fills, uniforms, white });
                self.program.write_frame(self.viewport, &mut orders);
                self.fresh = Fresh::Stale;
            }
            refused => self.unmade_program("white pixel", &refused, &mut orders),
        }

        orders
    }

    /// A font's metrics came, as blobs `read` reads, or will not: read, they are kept for good; either way, the next
    /// `Prepare` lays the screen out.
    pub fn metrics<'a>(
        &mut self,
        answered: Answered<FontMetrics>,
        MetricsAsked { font }: MetricsAsked,
        read: impl Fn(&Bytes) -> &'a [u8],
    ) -> Orders {
        let mut orders = Orders::default();
        let decoded = match answered {
            Answered::Yes(FontMetrics::Read { own, fallbacks }) => {
                let own = Metrics::decode(read(&own));
                let fallbacks: Result<Vec<Metrics>, _> =
                    fallbacks.iter().map(|blob| Metrics::decode(read(blob))).collect();
                own.and_then(|own| Ok((own, fallbacks?))).map_err(|malformed| format!("{malformed:?}"))
            }
            refused => Err(format!("{refused:?}")),
        };
        match decoded {
            Ok((own, fallbacks)) => self.metrics_read(font, own, fallbacks),
            Err(why) => {
                orders.log(
                    Level::Error,
                    format!("the metrics of {font:?} did not come, so its text draws nothing: {why}"),
                );
                self.metrics_refused(font);
            }
        }

        orders
    }
}
