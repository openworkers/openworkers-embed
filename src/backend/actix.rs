//! Actix Web.

use super::set_client;
use crate::Embed;
use crate::response::text;
use actix_web::{HttpRequest as ActixRequest, HttpResponse as ActixResponse, web};
use openworkers_core::HttpRequest;

/// Send every request to the worker:
///
/// ```ignore
/// App::new()
///     .app_data(web::Data::new(embed))
///     .default_service(web::to(openworkers_embed::backend::actix::handle))
/// ```
///
/// The worker gets the address of the peer in `x-real-ip`, and the scheme of
/// the server in `x-forwarded-proto`.
///
/// Actix reads at most 256 KiB of body by default; raise it with `web::PayloadConfig`.
pub async fn handle(
    embed: web::Data<Embed>,
    request: ActixRequest,
    body: web::Bytes,
) -> ActixResponse {
    let path = request.path().to_string();
    let scheme = if request.app_config().secure() {
        "https"
    } else {
        "http"
    };
    let mut worker_request = HttpRequest::from_actix(&request, body);

    worker_request.url = url(&request, scheme);

    set_client(
        &mut worker_request,
        request.peer_addr().map(|peer| peer.ip()),
        scheme,
    );

    match embed.fetch_at(worker_request, &path).await {
        Ok(response) => response.into(),
        Err(error) => text(502, &error.to_string()).into(),
    }
}

/// The URL as the hyper backend makes it: the scheme of the server, the Host
/// header (or the host of the server config) and the path with the query.
/// `connection_info` of actix is not used: it reads the scheme and the host
/// from the `Forwarded` and `X-Forwarded-*` headers of the client.
fn url(request: &ActixRequest, scheme: &str) -> String {
    let host = request
        .headers()
        .get(actix_web::http::header::HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or_else(|| request.app_config().host());
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str());

    format!("{scheme}://{host}{path}")
}
