//! Any hyper 1 server.

use super::set_client;
use crate::Embed;
use crate::response::text;
use http_body_util::{BodyExt, Limited};
use hyper::body::Body;
use openworkers_core::HttpRequest;
use openworkers_core::HyperBody;
use std::error::Error;
use std::net::SocketAddr;

const MAX_REQUEST_BODY: usize = 10 * 1024 * 1024;

/// Answer one request from `peer`, the address the connection comes from.
/// Call it from a `hyper::service::service_fn`; the request is plain HTTP, so
/// terminate TLS before it. The worker gets `peer` in `x-real-ip`.
pub async fn handle<B>(
    embed: &Embed,
    request: hyper::Request<B>,
    peer: SocketAddr,
) -> hyper::Response<HyperBody>
where
    B: Body,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    forward(embed, request, Some(peer)).await
}

/// [`handle`], with no `x-real-ip` when the peer is not known.
pub(crate) async fn forward<B>(
    embed: &Embed,
    request: hyper::Request<B>,
    peer: Option<SocketAddr>,
) -> hyper::Response<HyperBody>
where
    B: Body,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    let (parts, body) = request.into_parts();

    let Ok(body) = Limited::new(body, MAX_REQUEST_BODY).collect().await else {
        return text(413, "the request body is too large or unreadable").into_hyper();
    };

    let mut request = HttpRequest::from_hyper_parts(
        &parts.method,
        &parts.uri,
        &parts.headers,
        body.to_bytes(),
        "http",
    );

    set_client(&mut request, peer.map(|peer| peer.ip()), "http");

    match embed.fetch_at(request, parts.uri.path()).await {
        Ok(response) => response.into_hyper(),
        Err(error) => text(502, &error.to_string()).into_hyper(),
    }
}
