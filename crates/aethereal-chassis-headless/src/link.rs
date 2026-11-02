//! The link's hub end on the native host: the system's randomness, which `Engines` mints its tokens from, and its
//! bootstrap, which serves it (ADR-0034, ADR-0040).

use crate::Headless;
use aethereal_actor::Service;
use aethereal_link::{Engines, EnginesSettings, Randomness, Unfilled};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use core::error::Error;
use core::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::sync::Arc;

/// Where the system keeps its randomness: the kernel's generator, which never blocks once the machine has started.
const SYSTEM_RANDOMNESS: &str = "/dev/urandom";

/// The system's randomness (ADR-0040): the kernel's generator, read through a file opened once. A read of a few bytes
/// is answered at once, so it never blocks the thread that polls a job.
pub struct SystemRandomness {
    file: File,
}

/// Why the system's randomness did not open: the host has no `/dev/urandom`, or it would not open.
#[derive(Debug)]
pub struct NoRandomness {
    error: io::Error,
}

impl SystemRandomness {
    /// The system's randomness, opened.
    pub fn open() -> Result<Self, NoRandomness> {
        let file = File::open(SYSTEM_RANDOMNESS).map_err(|error| NoRandomness { error })?;

        Ok(Self { file })
    }
}

/// Reads through a shared reference: the kernel serves each read whole, whichever thread asks, so no lock is needed.
impl Randomness for SystemRandomness {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), Unfilled> {
        (&self.file).read_exact(bytes).map_err(|_| Unfilled)
    }
}

impl fmt::Display for NoRandomness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the system's randomness, `{SYSTEM_RANDOMNESS}`, did not open, so no token can be minted: {}",
            self.error
        )
    }
}

impl Error for NoRandomness {}

/// The link's hub end on the native host: its tokens minted from the system's randomness, served to the host for its
/// runners, and refused where a frame its engines would read could not be read.
impl Bootstrap<Headless> for Engines {
    const SECTION: &'static str = "engines";
    type Settings = EnginesSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: EnginesSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        settings.check_read_limit(cx.read_limit()).map_err(|why| Unbuilt::Failed(Box::new(why)))?;
        let randomness = SystemRandomness::open().map_err(|why| Unbuilt::Failed(Box::new(why)))?;
        cx.serve(Service::<dyn Randomness>::of(Arc::new(randomness)));

        cx.begin(&(), settings)
    }
}
