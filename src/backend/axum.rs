//! Axum.

use super::hyper;
use super::{Connection, Scheme};
use crate::Embed;
use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::Response;
use openworkers_core::HyperBody;
use std::net::SocketAddr;
use std::sync::Arc;

/// A router that sends every request to the worker. Mount it as is, or `nest`
/// it behind your own routes.
///
/// Serve it with `into_make_service_with_connect_info::<SocketAddr>()` for the
/// worker to get the address of the client in `x-real-ip`; without it, the
/// worker gets no address. The scheme is `http`: [`router_with`] sets it.
pub fn router(embed: Embed) -> Router {
    router_with(embed, Scheme::Http)
}

/// [`router`], with the scheme the clients use: `Https` behind a proxy that
/// ends TLS.
pub fn router_with(embed: Embed, scheme: Scheme) -> Router {
    Router::new()
        .fallback(handle)
        .with_state((Arc::new(embed), scheme))
}

async fn handle(
    State((embed, scheme)): State<(Arc<Embed>, Scheme)>,
    request: Request,
) -> Response<HyperBody> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);

    hyper::handle_with(&embed, request, Connection { peer, scheme }).await
}
