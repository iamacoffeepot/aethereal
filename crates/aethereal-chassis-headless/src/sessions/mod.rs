//! The sessions capability on the native host (ADR-0057): its records and transcripts are kept through `Files`, each
//! home is a volume Docker keeps, and each drop box is the store's, so what the host serves it is the read of the
//! credential's file and the system's randomness, which each session's bearer is drawn from.

use crate::Headless;
use crate::link::SystemRandomness;
use aethereal_actor::{Randomness, Service};
use aethereal_sessions::{Credential, NoCredential, Sessions, SessionsSettings};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// The credential's file, where the settings name it, read whole as each container is made. It is a secret, so it is
/// read here, by the host, and never through `Files`, whose namespaces any actor may read; its bytes are handed only
/// to the job that turns them into the container's environment.
pub struct CredentialFile {
    path: PathBuf,
}

impl Credential for CredentialFile {
    fn read(&self) -> Result<Vec<u8>, NoCredential> {
        fs::read(&self.path).map_err(|_| NoCredential)
    }
}

/// The sessions capability on the native host: the credential's file its settings name and the system's randomness,
/// served to the host for its runners (ADR-0034, ADR-0057). It refuses to start when a label of its settings is not
/// `key=value`, or the system's randomness does not open.
impl Bootstrap<Headless> for Sessions {
    const SECTION: &'static str = "sessions";
    type Settings = SessionsSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: SessionsSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        settings.container_labels().map_err(|why| Unbuilt::Failed(Box::new(why)))?;
        let credential = CredentialFile { path: PathBuf::from(&settings.credential) };
        cx.serve(Service::<dyn Credential>::of(Arc::new(credential)));
        let randomness = SystemRandomness::open().map_err(|why| Unbuilt::Failed(Box::new(why)))?;
        cx.serve(Service::<dyn Randomness>::of(Arc::new(randomness)));
        cx.begin(&(), settings)?;

        Ok(())
    }
}
