//! Any hyper 1 server.

use super::{Connection, set_client, url};
use crate::Embed;
use crate::response::text;
use http_body_util::{BodyExt, Limited};
use hyper::body::Body;
use openworkers_core::HttpRequest;
use openworkers_core::HyperBody;
use std::error::Error;

const MAX_REQUEST_BODY: usize = 10 * 1024 * 1024;

/// Answer one request. Call it from a `hyper::service::service_fn`; the
/// request is plain HTTP, so terminate TLS before it. The worker gets no
/// client address and the scheme `http`: [`handle_with`] gives them.
pub async fn handle<B>(embed: &Embed, request: hyper::Request<B>) -> hyper::Response<HyperBody>
where
    B: Body,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    handle_with(embed, request, Connection::default()).await
}

/// [`handle`], with the connection the request came on: the worker gets its
/// peer in `x-real-ip` and its scheme in `x-forwarded-proto` and the URL.
pub async fn handle_with<B>(
    embed: &Embed,
    request: hyper::Request<B>,
    connection: Connection,
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
        connection.scheme.as_str(),
    );

    // The Host header, else the authority of the URI (HTTP/2)
    let authority = parts
        .headers
        .get(hyper::header::HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| parts.uri.authority().map(|authority| authority.as_str()))
        .unwrap_or("localhost");

    request.url = url(
        connection.scheme,
        authority,
        parts.uri.path_and_query().map(|path| path.as_str()),
    );

    set_client(&mut request, connection);

    match embed.fetch_at(request, parts.uri.path()).await {
        Ok(response) => response.into_hyper(),
        Err(error) => text(502, &error.to_string()).into_hyper(),
    }
}
