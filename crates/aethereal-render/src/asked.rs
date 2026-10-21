//! What the checks' tests ask with: a request whose WGSL is a blob, which a check never reads, as it is handed the
//! text the blob holds apart.

use aethereal_actor::{BlobRef, Puts};
use core::ops::{Deref, DerefMut};

/// A blob holding nothing, for a request whose check is handed its WGSL apart.
pub fn blob() -> BlobRef {
    let Ok(blob) = Puts::new(0, 0, 0).pinned(Vec::new()) else {
        panic!("no bytes fit in no room");
    };

    blob
}

/// A request, `create`, and the WGSL its blob would hold, `wgsl`, which its check is handed. It reads as its request,
/// but for its WGSL, so a test changes either as it would a request whose WGSL is text.
#[derive(Clone)]
pub struct Asked<C> {
    pub create: C,
    pub wgsl: String,
}

impl<C> Asked<C> {
    /// The request `create`, of the WGSL `wgsl`.
    pub fn new(create: C, wgsl: &str) -> Self {
        Self { create, wgsl: String::from(wgsl) }
    }
}

impl<C> Deref for Asked<C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.create
    }
}

impl<C> DerefMut for Asked<C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.create
    }
}
