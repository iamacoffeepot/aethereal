//! What a request asks of the repository in git's smart HTTP (`gitprotocol-http`): the refs a service advertises, or
//! one exchange of it. Only these reach the program, and only as this crate names them: nothing of the request's path or
//! query is handed on as it was sent.

use aethereal_data::Plain;
use aethereal_http::{Method, Status};

/// One of git's two services over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.service")]
pub enum Service {
    /// A fetch or a clone: the repository's objects sent to the client.
    UploadPack,
    /// A push: the client's objects and ref updates taken into the repository.
    ReceivePack,
}

impl Service {
    /// The service's name, as git's client writes it.
    const fn name(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }

    /// The service `name` names: none for any other.
    fn named(name: &str) -> Option<Self> {
        [Self::UploadPack, Self::ReceivePack].into_iter().find(|service| service.name() == name)
    }
}

/// What a request asks of the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.call")]
pub enum Call {
    /// `GET .../info/refs?service=<service>`: the refs the service advertises, which a client reads first.
    Refs(Service),
    /// `POST .../<service>`: one exchange of the service, its request's body what the client sends it.
    Exchange(Service),
}

impl Call {
    /// The call a request of `method` on `path` with `query` makes, read from the last segments of its path, wherever
    /// the route that reached it stands.
    ///
    /// # Errors
    ///
    /// The status a request that makes none is answered: `404` for a path that is none of git's, or a method other
    /// than the one its path takes; `403` for the refs of anything but one of the two services, as git's dumb protocol
    /// asks them with no service, which is not served.
    pub fn of(method: Method, path: &str, query: &str) -> Result<Self, Status> {
        match method {
            Method::Get if path.ends_with("/info/refs") => {
                let service = query.strip_prefix("service=").and_then(Service::named).ok_or(Status::Forbidden)?;

                Ok(Self::Refs(service))
            }
            Method::Post => {
                let last = path.rsplit('/').next().unwrap_or_default();
                let service = Service::named(last).ok_or(Status::NotFound)?;

                Ok(Self::Exchange(service))
            }
            Method::Get | Method::Head | Method::Put | Method::Delete | Method::Patch | Method::Options => {
                Err(Status::NotFound)
            }
        }
    }

    /// The method git's client asks it with, as CGI's `REQUEST_METHOD` names it.
    pub(crate) const fn method(self) -> &'static str {
        match self {
            Self::Refs(_) => "GET",
            Self::Exchange(_) => "POST",
        }
    }

    /// The path `git http-backend` reads beneath the repository, as CGI's `PATH_INFO`.
    pub(crate) const fn path_info(self) -> &'static str {
        match self {
            Self::Refs(_) => "/info/refs",
            Self::Exchange(Service::UploadPack) => "/git-upload-pack",
            Self::Exchange(Service::ReceivePack) => "/git-receive-pack",
        }
    }

    /// The query `git http-backend` reads, as CGI's `QUERY_STRING`: the service whose refs are asked, or none.
    pub(crate) const fn query(self) -> &'static str {
        match self {
            Self::Refs(Service::UploadPack) => "service=git-upload-pack",
            Self::Refs(Service::ReceivePack) => "service=git-receive-pack",
            Self::Exchange(_) => "",
        }
    }
}

#[cfg(test)]
mod tests;
