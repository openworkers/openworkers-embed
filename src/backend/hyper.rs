//! Any hyper 1 server.

use crate::Embed;
use crate::response::text;
use http_body_util::{BodyExt, Limited};
use hyper::body::Body;
use openworkers_core::HttpRequest;
use openworkers_core::HyperBody;
use std::error::Error;

const MAX_REQUEST_BODY: usize = 10 * 1024 * 1024;

/// Answer one request. Call it from a `hyper::service::service_fn`; the
/// request is plain HTTP, so terminate TLS before it.
pub async fn handle<B>(embed: &Embed, request: hyper::Request<B>) -> hyper::Response<HyperBody>
where
    B: Body,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    let (parts, body) = request.into_parts();

    let Ok(body) = Limited::new(body, MAX_REQUEST_BODY).collect().await else {
        return text(413, "the request body is too large or unreadable").into_hyper();
    };

    let request = HttpRequest::from_hyper_parts(
        &parts.method,
        &parts.uri,
        &parts.headers,
        body.to_bytes(),
        "http",
    );

    match embed.fetch_at(request, parts.uri.path()).await {
        Ok(response) => response.into_hyper(),
        Err(error) => text(502, &error.to_string()).into_hyper(),
    }
}
