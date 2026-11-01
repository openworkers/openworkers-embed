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
