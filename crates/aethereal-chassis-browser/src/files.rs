//! The browser's storage: the program's assets fetched relative to the page, and its saves in the origin private file
//! system (ADR-0032, ADR-0039).

use aethereal_actor::Spares;
use aethereal_data::{FilePath, Plain};
use aethereal_files::{Namespace, Read, Reading, Storage, Unread, Unwritten, Writing};
use js_sys::{Promise, Uint8Array};
use std::future;
use std::iter;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DomException, File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemWritableFileStream, Response,
};

/// The directory of the origin private file system the program's saves are in, so each namespace written there has a
/// directory of its own.
const SAVES: &str = "save";

/// Where the browser reads each namespace from: the file capability's section, `files` (ADR-0028, ADR-0032). Its saves
/// are the origin's, so they have nothing to set (ADR-0039).
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

/// The browser's storage (ADR-0032, ADR-0039): each asset fetched from beneath its URL, and each save read and written
/// in the origin private file system, whose writable stream replaces a file whole as it is closed. Each is a promise
/// the page's event loop drives. A page served beside its assets, an artifact among them, reads them with no setting.
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
}

impl Storage for Fetching {
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath, lent: Spares) -> Reading<'a> {
        Box::pin(async move {
            let array = match namespace {
                Namespace::Assets => fetch(format!("{}/{path}", self.assets)).await,
                Namespace::Save => read_saved(path).await,
            };

            filled(array, lent)
        })
    }

    fn write<'a>(&'a self, namespace: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        match namespace {
            Namespace::Assets => Box::pin(future::ready(Err(Unwritten::ReadOnly))),
            Namespace::Save => Box::pin(write_saved(path, bytes)),
        }
    }
}

/// What a read that came to `array` comes to: its bytes copied into a buffer taken from `lent` for their size, which
/// is one `lent` holds when it fits them, and what is left of `lent` (ADR-0042).
fn filled(array: Result<Uint8Array, Unread>, mut lent: Spares) -> Read {
    let bytes = array.and_then(|array| {
        let size = usize::try_from(array.length()).map_err(|_| Unread::Failed)?;
        let mut bytes = lent.take(size);
        bytes.resize(size, 0);
        array.copy_to(&mut bytes);

        Ok(bytes)
    });

    Read { bytes, lent }
}

/// The bytes at `url`: missing when the server answers it is not found, and failed when the fetch cannot be made or
/// the server answers anything else but success.
async fn fetch(url: String) -> Result<Uint8Array, Unread> {
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

    Ok(Uint8Array::new(&buffer))
}

/// Why the origin private file system did not do what was asked, as the exception it rejected with names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// Nothing is at a path, or a file is where a directory was asked for, or a directory where a file was.
    Missing,
    /// The browser gives this page no storage, as a private window or a sandboxed frame may.
    Unavailable,
    /// Anything else.
    Failed,
}

impl From<Refused> for Unread {
    fn from(refused: Refused) -> Self {
        match refused {
            Refused::Missing => Self::Missing,
            Refused::Unavailable => Self::Unavailable,
            Refused::Failed => Self::Failed,
        }
    }
}

/// A write that found a file where a directory was asked for, or a directory where its file was, could not be done.
impl From<Refused> for Unwritten {
    fn from(refused: Refused) -> Self {
        match refused {
            Refused::Unavailable => Self::Unavailable,
            Refused::Missing | Refused::Failed => Self::Failed,
        }
    }
}

/// Why `error`, a promise's rejection, refused.
fn refused(error: &JsValue) -> Refused {
    let name = error.dyn_ref::<DomException>().map(DomException::name);
    match name.as_deref() {
        Some("NotFoundError" | "TypeMismatchError") => Refused::Missing,
        Some("SecurityError" | "NotAllowedError") => Refused::Unavailable,
        _ => Refused::Failed,
    }
}

/// What `promise` resolves to, as a `T`.
async fn resolved<T: JsCast>(promise: Promise) -> Result<T, Refused> {
    let value = JsFuture::from(promise).await.map_err(|error| refused(&error))?;

    value.dyn_into().map_err(|_| Refused::Failed)
}

/// The directory of the saves the save at `path` is in, each directory on the way made when `make` says so, and the
/// file's name in it.
async fn directory_of(path: &FilePath, make: bool) -> Result<(FileSystemDirectoryHandle, &str), Refused> {
    let window = web_sys::window().ok_or(Refused::Unavailable)?;
    let options = FileSystemGetDirectoryOptions::new();
    options.set_create(make);
    let (folders, name) = path.as_str().rsplit_once('/').unwrap_or(("", path.as_str()));
    let mut directory: FileSystemDirectoryHandle = resolved(window.navigator().storage().get_directory()).await?;
    for folder in iter::once(SAVES).chain(folders.split('/').filter(|folder| !folder.is_empty())) {
        directory = resolved(directory.get_directory_handle_with_options(folder, &options)).await?;
    }

    Ok((directory, name))
}

/// The whole of the save at `path`.
async fn read_saved(path: &FilePath) -> Result<Uint8Array, Unread> {
    let (directory, name) = directory_of(path, false).await?;
    let handle: FileSystemFileHandle = resolved(directory.get_file_handle(name)).await?;
    let file: File = resolved(handle.get_file()).await?;
    let buffer = JsFuture::from(file.array_buffer()).await.map_err(|_| Unread::Failed)?;

    Ok(Uint8Array::new(&buffer))
}

/// Writes `bytes` as the whole of the save at `path`, making the directories it names. The writable stream writes
/// beside the file and replaces it as it is closed, so a reader never sees half of it; one that could not be written
/// is aborted, and the file is left as it was.
async fn write_saved(path: &FilePath, bytes: Vec<u8>) -> Result<(), Unwritten> {
    let (directory, name) = directory_of(path, true).await?;
    let options = FileSystemGetFileOptions::new();
    options.set_create(true);
    let handle: FileSystemFileHandle = resolved(directory.get_file_handle_with_options(name, &options)).await?;
    let stream: FileSystemWritableFileStream = resolved(handle.create_writable()).await?;
    let writing = stream.write_with_u8_array(&bytes).map_err(|error| refused(&error))?;
    let written = JsFuture::from(writing).await;
    if written.is_err() {
        // The stream's file is left as it was; what the abort answers changes nothing of it.
        let _ = JsFuture::from(stream.abort()).await;
        return Err(Unwritten::Failed);
    }
    JsFuture::from(stream.close()).await.map_err(|error| Unwritten::from(refused(&error)))?;

    Ok(())
}
