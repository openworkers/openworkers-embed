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
use std::net::IpAddr;

#[cfg(any(feature = "actix", feature = "hyper"))]
const REAL_IP: &str = "x-real-ip";
#[cfg(any(feature = "actix", feature = "hyper"))]
const FORWARDED_PROTO: &str = "x-forwarded-proto";

/// Give the worker its client as the proxy in front of the runner does:
/// `x-real-ip` is the address of the peer and `x-forwarded-proto` the scheme
/// of the listener, which also starts the URL. A value the client sent is
/// replaced; `x-real-ip` is removed when the peer is not known.
#[cfg(any(feature = "actix", feature = "hyper"))]
pub(crate) fn set_client(request: &mut HttpRequest, peer: Option<IpAddr>, scheme: &str) {
    request.headers.retain(|name, _| {
        !name.eq_ignore_ascii_case(REAL_IP) && !name.eq_ignore_ascii_case(FORWARDED_PROTO)
    });

    if let Some(peer) = peer {
        request
            .headers
            .insert(REAL_IP.to_string(), peer.to_canonical().to_string());
    }

    request
        .headers
        .insert(FORWARDED_PROTO.to_string(), scheme.to_string());

    if let Some((_, rest)) = request.url.split_once("://") {
        request.url = format!("{scheme}://{rest}");
    }
}
