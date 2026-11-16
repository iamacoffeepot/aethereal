//! The native host's disk for the sessions capability: each session a directory under the root its settings name,
//! worked on a thread of its own (ADR-0057).

mod disk;

use crate::Headless;
use crate::errands::Errands;
use aethereal_actor::Service;
use aethereal_sessions::{Fetching, Finding, Making, Sessions, SessionsDisk, SessionsSettings, Storing, Unkept};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use std::path::PathBuf;
use std::sync::Arc;

/// The native host's disk for the sessions (ADR-0057): the root its settings name, each session's directory under it
/// named by its number, holding its `record`, its `transcript`, and its `home`; and the credential's file, read where
/// the settings name it. What it is asked is done on a thread of its own, in the order asked, so the disk never blocks
/// the thread that polls it.
pub struct SessionsDirectory {
    root: PathBuf,
    credential: PathBuf,
    errands: Errands,
}

impl SessionsDirectory {
    /// The disk keeping the sessions where `settings` say.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(settings: &SessionsSettings) -> Self {
        Self {
            root: PathBuf::from(&settings.root),
            credential: PathBuf::from(&settings.credential),
            errands: Errands::new("aethereal-sessions"),
        }
    }
}

impl SessionsDisk for SessionsDirectory {
    fn find(&self) -> Finding<'_> {
        let root = self.root.clone();
        let finding = self.errands.ask(move || disk::find(&root));

        Box::pin(async move { finding.await.unwrap_or(Err(Unkept)) })
    }

    fn make(&self, session: u32) -> Making<'_> {
        let root = self.root.clone();
        let making = self.errands.ask(move || disk::make(&root, session));

        Box::pin(async move { making.await.unwrap_or(Err(Unkept)) })
    }

    fn credential(&self) -> Fetching<'_> {
        let credential = self.credential.clone();
        let fetching = self.errands.ask(move || disk::credential(&credential));

        Box::pin(async move { fetching.await.unwrap_or(Err(Unkept)) })
    }

    fn keep(&self, session: u32, frames: Vec<u8>, record: Vec<u8>) -> Storing<'_> {
        let root = self.root.clone();
        let storing = self.errands.ask(move || disk::keep(&root, session, &frames, &record));

        Box::pin(async move { storing.await.unwrap_or(Err(Unkept)) })
    }
}

/// The sessions capability on the native host: its disk the directory its settings name, served to the host for its
/// runners (ADR-0057). It refuses to start when a label of its settings is not `key=value`.
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
        cx.serve(Service::<dyn SessionsDisk>::of(Arc::new(SessionsDirectory::new(&settings))));
        cx.begin(&(), settings)
    }
}
