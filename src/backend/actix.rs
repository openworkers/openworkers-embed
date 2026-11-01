//! Actix Web.

use super::{Connection, Scheme, set_client, url};
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
/// The worker gets the address of the peer in `x-real-ip`, and the scheme in
/// `x-forwarded-proto` and the URL: `https` when the server ends TLS itself,
/// or when the app holds `Scheme::Https` as app data (behind a proxy that
/// ends TLS: `App::new().app_data(Scheme::Https)`).
///
/// Actix reads at most 256 KiB of body by default; raise it with `web::PayloadConfig`.
pub async fn handle(
    embed: web::Data<Embed>,
    request: ActixRequest,
    body: web::Bytes,
) -> ActixResponse {
    let path = request.path().to_string();
    let connection = Connection {
        peer: request.peer_addr(),
        scheme: scheme(request.app_config().secure(), request.app_data::<Scheme>()),
    };
    let mut worker_request = HttpRequest::from_actix(&request, body);

    // The Host header, else the authority of the URI (HTTP/2), else the host
    // of the server config
    let authority = request
        .headers()
        .get(actix_web::http::header::HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.as_str())
        })
        .unwrap_or_else(|| request.app_config().host());

    worker_request.url = url(
        connection.scheme,
        authority,
        request.uri().path_and_query().map(|path| path.as_str()),
    );

    set_client(&mut worker_request, connection);

    match embed.fetch_at(worker_request, &path).await {
        Ok(response) => response.into(),
        Err(error) => text(502, &error.to_string()).into(),
    }
}

/// `https` when the server ends TLS, else the scheme the app declares.
fn scheme(secure: bool, declared: Option<&Scheme>) -> Scheme {
    if secure {
        return Scheme::Https;
    }

    declared.copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Scheme, scheme};

    // A test cannot make a secure actix server without TLS: the rule is tested
    // on its own
    #[test]
    fn a_secure_server_is_https_whatever_the_app_declares() {
        assert_eq!(scheme(true, None), Scheme::Https);
        assert_eq!(scheme(true, Some(&Scheme::Http)), Scheme::Https);
        assert_eq!(scheme(false, Some(&Scheme::Https)), Scheme::Https);
        assert_eq!(scheme(false, None), Scheme::Http);
    }
}
