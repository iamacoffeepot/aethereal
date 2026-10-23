//! The browser's storage: the program's assets fetched relative to the page (ADR-0032).

use aethereal_actor::{Namespace, Reading, Storage, Unread};
use aethereal_data::{FilePath, Plain};
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::Response;

/// Where the browser reads each namespace from: the file capability's section, `files` (ADR-0028, ADR-0032).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.files.urls")]
#[settings]
pub struct FileUrls {
    /// The URL the program's assets are fetched beneath: relative to the page, or absolute.
    #[flag]
    pub assets: String,
}

impl Default for FileUrls {
    fn default() -> Self {
        Self { assets: String::from("assets") }
    }
}

/// The browser's storage (ADR-0032): each file of a namespace fetched from beneath its URL, a promise the page's
/// event loop drives. A page served beside its assets, an artifact among them, reads them with no setting.
pub struct Fetching {
    /// The URL of the program's assets, with no `/` at its end.
    assets: String,
}

impl Fetching {
    /// The storage fetching from beneath the URLs `urls` names.
    #[must_use]
    pub fn new(urls: &FileUrls) -> Self {
        Self { assets: String::from(urls.assets.trim_end_matches('/')) }
    }

    /// The URL `namespace` is fetched beneath.
    fn base(&self, namespace: Namespace) -> &str {
        match namespace {
            Namespace::Assets => &self.assets,
        }
    }
}

impl Storage for Fetching {
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath) -> Reading<'a> {
        let url = format!("{}/{path}", self.base(namespace));

        Box::pin(fetch(url))
    }
}

/// The bytes at `url`: missing when the server answers it is not found, and failed when the fetch cannot be made or
/// the server answers anything else but success.
async fn fetch(url: String) -> Result<Vec<u8>, Unread> {
    let window = web_sys::window().ok_or(Unread::Unavailable)?;
    let answered = JsFuture::from(window.fetch_with_str(&url)).await.map_err(|_| Unread::Failed)?;
    let response: Response = answered.dyn_into().map_err(|_| Unread::Failed)?;
    let is_missing = response.status() == 404;
    if is_missing {
        return Err(Unread::Missing);
    }
    if !response.ok() {
        return Err(Unread::Failed);
    }
    let body = response.array_buffer().map_err(|_| Unread::Failed)?;
    let buffer = JsFuture::from(body).await.map_err(|_| Unread::Failed)?;

    Ok(Uint8Array::new(&buffer).to_vec())
}
