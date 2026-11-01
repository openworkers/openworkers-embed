//! Web frameworks `openworkers-core` converts requests and responses for.
//! Each one is behind a feature of the same name.

#[cfg(feature = "actix")]
pub mod actix;
#[cfg(feature = "axum")]
pub mod axum;
#[cfg(feature = "hyper")]
pub mod hyper;

#[cfg(any(feature = "actix", feature = "hyper"))]
use openworkers_core::HttpRequest;
#[cfg(any(feature = "actix", feature = "hyper"))]
use std::net::SocketAddr;

/// The scheme the client used. A backend cannot see TLS that a proxy ends in
/// front of it: behind such a proxy, the host says `Https`.
#[cfg(any(feature = "actix", feature = "hyper"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Http,
    Https,
}

#[cfg(any(feature = "actix", feature = "hyper"))]
impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// The connection a request came on, as the host knows it. The default has no
/// peer, so the worker gets no address, and the scheme `Http`.
#[cfg(any(feature = "actix", feature = "hyper"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    /// The address of the client. Behind a proxy, this is the proxy.
    pub peer: Option<SocketAddr>,
    pub scheme: Scheme,
}

/// The headers that only the runner sets, as `RUNNER_HEADERS` of
/// openworkers-runner: a request from a client loses them.
#[cfg(any(feature = "actix", feature = "hyper"))]
const RUNNER_HEADERS: [&str; 12] = [
    "x-worker-id",
    "x-worker-name",
    "x-openworkers-internal",
    "x-openworkers-depth",
    "x-request-id",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "x-real-ip",
    "cf-connecting-ip",
];

/// The headers that carry the address of the client, as the runner sets them.
#[cfg(any(feature = "actix", feature = "hyper"))]
const CLIENT_HEADERS: [&str; 3] = ["x-real-ip", "x-forwarded-for", "cf-connecting-ip"];

/// Give the worker its client as the runner does: the headers of the runner
/// that the client sent are removed; the address of the peer goes in
/// `x-real-ip`, `x-forwarded-for` and `cf-connecting-ip`, and the scheme of the
/// listener in `x-forwarded-proto` and at the start of the URL. With no peer,
/// the worker gets no address.
#[cfg(any(feature = "actix", feature = "hyper"))]
pub(crate) fn set_client(request: &mut HttpRequest, connection: Connection) {
    let scheme = connection.scheme.as_str();

    request.headers.retain(|name, _| {
        !RUNNER_HEADERS
            .iter()
            .any(|header| name.eq_ignore_ascii_case(header))
    });

    if let Some(peer) = connection.peer {
        let peer = peer.ip().to_canonical().to_string();

        for header in CLIENT_HEADERS {
            request.headers.insert(header.to_string(), peer.clone());
        }
    }

    request
        .headers
        .insert("x-forwarded-proto".to_string(), scheme.to_string());

    if let Some((_, rest)) = request.url.split_once("://") {
        request.url = format!("{scheme}://{rest}");
    }
}
