//! Actix Web.

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
/// Actix reads at most 256 KiB of body by default; raise it with `web::PayloadConfig`.
pub async fn handle(
    embed: web::Data<Embed>,
    request: ActixRequest,
    body: web::Bytes,
) -> ActixResponse {
    match embed.fetch(HttpRequest::from_actix(&request, body)).await {
        Ok(response) => response.into(),
        Err(error) => text(502, &error.to_string()).into(),
    }
}
